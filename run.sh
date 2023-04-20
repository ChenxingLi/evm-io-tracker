cargo build --release
for i in `seq 13504580 10 13510000`; 
do
    ./target/release/amt-trace $i;
done