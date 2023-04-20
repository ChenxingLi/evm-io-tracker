mod opcode;
mod parse;

use parse::parse_block_trace;

use ethers::{
    providers::{Http, Provider},
    types::{Address, U256},
};
use postcard::{from_bytes, to_stdvec};
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Write;
use std::{
    collections::{HashMap, HashSet},
    convert::TryFrom,
};
use tiny_keccak::{Hasher, Keccak};
use tokio::{
    task::JoinSet,
    time::{sleep, Instant},
};

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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ExperimentTask {
    Read([u8; 32]),
    Write([u8; 32], Vec<u8>),
}

async fn parse_main(number: usize) {
    const BATCH_SIZE: usize = 10;

    let provider = Provider::<Http>::try_from("http://localhost:8545")
        .expect("could not instantiate HTTP Provider");

    let mut set = JoinSet::new();

    let start = Instant::now();
    let mut answers: Vec<Vec<DBAccess>> = vec![Default::default(); BATCH_SIZE];
    for x in 0..BATCH_SIZE {
        let provider = provider.clone();
        let number = number + x;
        set.spawn(async move { (parse_block_trace(provider, number).await, x) });
    }

    let mut accesses_cnt = 0;
    while let Some(results) = set.join_next().await {
        let (accesses, x) = results.unwrap();
        accesses_cnt += accesses.len();
        answers[x] = accesses;
    }

    write_to_file(&answers, format!("data/{}.trace", number));
    let elapsed = start.elapsed();

    println!(
        "Block number {} to {}: {} items ({:?})",
        number,
        number + BATCH_SIZE - 1,
        accesses_cnt,
        elapsed
    );

    std::mem::drop(provider);
    sleep(elapsed / 3).await;
}

fn combine(numbers: impl Iterator<Item = usize>) {
    let mut combined_answer: Vec<Vec<DBAccess>> = Vec::new();
    for number in numbers {
        let answer: Vec<Vec<DBAccess>> = read_from_file(format!("data/{}.trace", number));
        combined_answer.extend(answer);
    }

    write_to_file(&combined_answer, "data/overall.trace");
}

fn write_to_file<T: Serialize, S: AsRef<str>>(data: &T, path: S) {
    let raw = to_stdvec(data).unwrap();
    File::create(path.as_ref())
        .unwrap()
        .write_all(&raw)
        .unwrap();
}

fn read_from_file<T, S: AsRef<str>>(path: S) -> T
where
    for<'a> T: Deserialize<'a>,
{
    let loaded = std::fs::read(path.as_ref()).unwrap();
    from_bytes(&loaded).unwrap()
}

fn u256_to_bytes(number: &U256) -> Vec<u8> {
    let mut encoded = [0u8; 32];
    number.to_big_endian(&mut encoded);
    encoded.to_vec()
}

fn make_task() {
    let loaded = std::fs::read("data/overall.trace").unwrap();
    let answer: Vec<Vec<DBAccess>> = from_bytes(&loaded).unwrap();

    let mut frontier = HashMap::<SlotKey, U256>::new();
    let mut touched = HashSet::<SlotKey>::new();
    let mut access_stat = HashMap::<SlotKey, (usize, usize)>::new();

    for x in answer.iter().flatten() {
        match &x {
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
        "Blocks {}, ops {}",
        answer.len(),
        answer.iter().map(|x| x.len()).sum::<usize>()
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
            block.into_iter().for_each(|x| match &x {
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

    write_to_file(&init_task, "data/real_trace.init");
    write_to_file(&io_task, "data/real_trace.data");

    // let mut stat_vec = access_stat.iter().collect::<Vec<_>>();
    // stat_vec.sort_unstable_by_key(|(_, (x, y))| x + y);

    // for (slot, (reads, writes)) in stat_vec.iter().rev().take(1000) {
    //     println!("{:?}, {} r {} w", slot, reads, writes);
    // }
}

#[tokio::main]
async fn main() {
    let args = std::env::args().collect::<Vec<String>>();
    if args.len() > 1 {
        let number = std::env::args().collect::<Vec<String>>()[1]
            .parse::<usize>()
            .unwrap();
        parse_main(number).await;
    } else {
        combine((13500000..=13504670).step_by(10));
        make_task();
    }
}
