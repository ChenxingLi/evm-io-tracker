mod opcode;
mod parse;

use opts::{CombineOptions, FetchOptions, SealOptions};
use parse::parse_block_trace;

use ethers::{
    providers::{Http, Provider},
    types::{Address, U256},
};
use postcard::{from_bytes, to_stdvec};
use rand::seq::SliceRandom;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::fs::File;
use std::{
    collections::{HashMap, HashSet},
    convert::TryFrom,
};
use std::{io::Write, path::Path};
use structopt::StructOpt;
use tiny_keccak::{Hasher, Keccak};
use tokio::{task::JoinSet, time::Instant};

use crate::opts::Options;
mod opts;

#[derive(Default, Clone, Debug, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct SlotKey {
    pub address: Address,
    pub slot: U256,
}

impl SlotKey {
    fn digest(&self) -> [u8; 32] {
        let mut output = [0u8; 32];
        let mut hasher = Keccak::v256();
        hasher.update(self.address.as_ref());
        let mut encoded = [0u8; 32];
        self.slot.to_big_endian(&mut encoded);
        hasher.update(&encoded);
        hasher.finalize(&mut output);
        output
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum DBAccess {
    Read(SlotKey, U256),
    Write(SlotKey, U256),
}

pub type TransactionAccess = Vec<DBAccess>;

pub type BlockAccess = Vec<TransactionAccess>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ExperimentTask {
    Read([u8; 32]),
    Write([u8; 32], Vec<u8>),
}

async fn fetch_main(opts: &FetchOptions) {
    let number = opts.start_block;
    let batch_size = opts.batch_size;

    let provider = Provider::<Http>::try_from(opts.node_url.clone())
        .expect("could not instantiate HTTP Provider");

    let mut set = JoinSet::new();

    let start = Instant::now();
    let mut answers: Vec<BlockAccess> = vec![Default::default(); batch_size];
    for x in 0..batch_size {
        let provider = provider.clone();
        let number = number + x;
        set.spawn(async move { (parse_block_trace(provider, number).await, x) });
    }

    let mut accesses_cnt: usize = 0;
    while let Some(results) = set.join_next().await {
        let (accesses, x) = results.unwrap();
        accesses_cnt += accesses.iter().map(|x| x.len()).sum::<usize>();
        answers[x] = accesses;
    }

    write_to_file(&answers, format!("data/{}_{}.trace", number, batch_size));
    let elapsed = start.elapsed();

    println!(
        "Block number {} to {}: {} items ({:?})",
        number,
        number + batch_size - 1,
        accesses_cnt,
        elapsed
    );

    std::mem::drop(provider);
    // sleep(std::cmp::min(elapsed / 3, Duration::from_secs(10))).await;
}

fn combine(opts: &CombineOptions) {
    let re = Regex::new(r"^(\d+)_(\d+)\.trace$").unwrap();
    let mut pathes: Vec<_> = fs::read_dir("data")
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| {
            let path = entry.path();
            if let Some(file_name) = path.file_name() {
                if let Some(captures) = re.captures(file_name.to_str().unwrap()) {
                    let start_number = captures[1].parse::<usize>().unwrap();
                    let length = captures[2].parse::<usize>().unwrap();

                    let start_cond = opts
                        .start_block
                        .map_or(true, |start| start_number + length > start);
                    let end_cond = opts.end_block.map_or(true, |end| start_number < end);
                    if start_cond && end_cond {
                        return Some((path, start_number, length));
                    }
                }
            }
            None
        })
        .collect();
    assert!(pathes.len() > 0, "Not found traces");
    pathes.sort_unstable_by_key(|(_, number, _)| *number);
    for i in 0..(pathes.len() - 1) {
        assert_eq!(
            pathes[i].1 + pathes[i].2,
            pathes[i + 1].1,
            "Provided files are not consecutive ranges: {} -> {}.",
            pathes[i].0.display(),
            pathes[i + 1].0.display()
        );
    }

    let mut combined_answer: Vec<BlockAccess> = Vec::new();
    let actual_start = opts.start_block.unwrap_or(pathes.first().unwrap().1);

    for (path, start_number, _) in pathes {
        let block_access_group: Vec<BlockAccess> = read_from_file(path);
        for (idx, block_access) in block_access_group.into_iter().enumerate() {
            let current_block = start_number + idx;
            if opts
                .start_block
                .map_or(true, |start| current_block >= start)
                && opts.end_block.map_or(true, |end| current_block < end)
            {
                combined_answer.push(block_access);
            }
        }
    }

    let output = Path::new(&opts.path).join(format!(
        "combined_{}_{}.trace",
        actual_start,
        combined_answer.len()
    ));

    write_to_file(&combined_answer, output);
}

fn write_to_file<T: Serialize, S: AsRef<Path>>(data: &T, path: S) {
    let raw = to_stdvec(data).unwrap();
    File::create(path.as_ref())
        .unwrap()
        .write_all(&raw)
        .unwrap();
}

fn read_from_file<T, S: AsRef<Path>>(path: S) -> T
where
    for<'a> T: Deserialize<'a>,
{
    let loaded = std::fs::read(path).unwrap();
    from_bytes(&loaded).unwrap()
}

fn u256_to_bytes(number: &U256) -> Vec<u8> {
    let mut encoded = [0u8; 32];
    number.to_big_endian(&mut encoded);
    encoded.to_vec()
}

fn seal(opts: &SealOptions) {
    let loaded = std::fs::read(&opts.input).unwrap();
    let answer: Vec<BlockAccess> = from_bytes(&loaded).unwrap();

    let mut frontier = HashMap::<SlotKey, U256>::new();
    let mut touched = HashSet::<SlotKey>::new();
    let mut access_stat = HashMap::<SlotKey, (usize, usize)>::new();

    for x in answer.iter().flatten().flatten() {
        match x {
            DBAccess::Read(slot, value) if !touched.contains(slot) => {
                touched.insert(slot.clone());
                if !value.is_zero() {
                    frontier.insert(slot.clone(), value.clone());
                }
                access_stat.entry(slot.clone()).or_default().0 += 1;
            }
            DBAccess::Read(slot, _) => {
                access_stat.entry(slot.clone()).or_default().0 += 1;
            }
            DBAccess::Write(slot, _) => {
                touched.insert(slot.clone());
                access_stat.entry(slot.clone()).or_default().1 += 1;
            }
        }
    }

    println!(
        "Blocks {}, txs {}, ops {}",
        answer.len(),
        answer.iter().map(|x| x.len()).sum::<usize>(),
        answer.iter().flatten().map(|x| x.len()).sum::<usize>()
    );
    println!("Touched set {}, init set {}", touched.len(), frontier.len());

    let mut init_task: Vec<_> = frontier
        .drain()
        .map(|(key, value)| (key.digest(), u256_to_bytes(&value)))
        .collect();
    init_task.shuffle(&mut rand::thread_rng());

    let io_task = answer
        .into_iter()
        .map(|block| {
            let mut ops = HashMap::<SlotKey, Vec<DBAccess>>::new();
            block.into_iter().flatten().for_each(|x| match &x {
                DBAccess::Read(slot, _) | DBAccess::Write(slot, _) => {
                    ops.entry(slot.clone()).or_insert(vec![]).push(x)
                }
            });

            let mut reads = Vec::new();
            let mut writes = Vec::new();

            for (_, rw_array) in ops {
                if let Some(DBAccess::Read(slot, _)) = rw_array.first() {
                    reads.push(ExperimentTask::Read(slot.digest()))
                }
                if let Some(DBAccess::Write(slot, value)) = rw_array
                    .iter()
                    .rev()
                    .filter(|x| matches!(x, DBAccess::Write(_, _)))
                    .next()
                {
                    writes.push(ExperimentTask::Write(slot.digest(), u256_to_bytes(&value)))
                }
            }

            let mut answer = reads;
            answer.extend_from_slice(&writes);
            answer
        })
        .collect::<Vec<_>>();

    let (read_cnt, write_cnt) =
        io_task
            .iter()
            .flatten()
            .fold((0usize, 0usize), |(r, w), x| match x {
                ExperimentTask::Read(..) => (r + 1, w),
                ExperimentTask::Write(..) => (r, w + 1),
            });
    println!("Final task {} r {} w", read_cnt, write_cnt);

    fs::create_dir_all(&opts.output).unwrap();

    write_to_file(&init_task, Path::new(&opts.output).join("real_trace.init"));
    write_to_file(&io_task, Path::new(&opts.output).join("real_trace.data"));

    // let mut stat_vec = access_stat.iter().collect::<Vec<_>>();
    // stat_vec.sort_unstable_by_key(|(_, (x, y))| x + y);

    // for (slot, (reads, writes)) in stat_vec.iter().rev().take(1000) {
    //     println!("{:?}, {} r {} w", slot, reads, writes);
    // }
}

#[tokio::main]
async fn main() {
    let options: Options = Options::from_args();
    match options {
        Options::Fetch(opts) => {
            fetch_main(&opts).await;
        }
        Options::Combine(opts) => {
            combine(&opts);
        }
        Options::Seal(opts) => {
            seal(&opts);
        }
    }
}
