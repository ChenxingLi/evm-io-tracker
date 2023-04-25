# EVM IO Tracker

## Introduction

This tool helps generate real trace evaluation tasks for [Authenticated Storage Benchmarks](https://github.com/ChenxingLi/authenticated-storage-benchmarks) by fetching I/O traces from an Ethereum archive node.

## Building the Project

To build the project on Ubuntu 22.04, follow these steps:

### Prerequisites

- Ubuntu 22.04
- Rust (version 1.67.0)
- Build tools: `build-essential`, `libssl-dev`, `pkg-config`, `libclang-dev`, `cmake`

### Steps

1. Update the package list:
    
    ```
    sudo apt update
    ```
    
2. Install Rust and Cargo:
    
    ```
    sudo apt install rustc cargo
    ```
    
3. Install additional dependencies:
    
    ```
    sudo apt install build-essential libssl-dev pkg-config libclang-dev cmake
    ```
    
4. Clone the repository and navigate to the project directory:
    
    ```
    git clone https://github.com/ChenxingLi/evm-io-tracker.git
    cd evm-io-tracker
    ```
    
5. Create a data directory:
    
    ```
    mkdir data
    ```
    
6. Build the project:
    
    ```
    cargo build --release
    ```

## Fetching and Parsing Traces

Now you can fetch traces from an Ethereum archive node and generate task files for [Authenticated Storage Benchmarks](https://github.com/ChenxingLi/authenticated-storage-benchmarks). 

1. Fetch traces from an Ethereum node:
        
    ```
    ./target/release/evm-io-tracker fetch --node-url <node-url> --start-block <block-number> --batch-size <batch-size>
    ```
    
    The `fetch` command fetches traces from an Ethereum archive node that supports the [trace API](https://openethereum.github.io/JSONRPC-trace-module). To use this command, you must provide the following arguments:
    
    - `node-url`: The URL of an Ethereum archive node that supports the trace API. You can use an archive node that supports [OpenEthereum's trace API](https://openethereum.github.io/JSONRPC-trace-module), or purchase a full node API service supporting trace API like [QuickNode](https://www.quicknode.com?tap_a=67226-09396e&tap_s=3826823-76f362&utm_source=affiliate&utm_campaign=generic&utm_content=affiliate_landing_page&utm_medium=generic).
    - `start-block`: The block number of the first block to fetch. This is the block from which the tracing will start.
    
    Additionally, you can specify an optional argument:
    
    - `batch-size`: The number of blocks to fetch. When using a local node, it is recommended to fetch fewer than 10 blocks at a time. When using an API service, a batch size of 50 blocks is recommended for better load balancing.
    
    Note that the `fetch` command fetches traces in batches. Each batch of blocks is saved in a separate file. The filenames of these files indicate the range of blocks they contain.
    
    You can use a bash script to fetch blocks in order:
    
    ```bash
    for i in `seq 13500000 50 13510000`;
    do
        ./target/release/evm-io-tracker fetch --node-url <node-url> --start-block $i --batch-size 50
    done
    ```
    
    The fetched traces will be saved in the `./data` folder.
    
2. Combine fetched data:
        
    ```bash
    ./target/release/evm-io-tracker combine --start-block <number> --end-block <number> --path <dir-path>
    ```
    
    This command will create a combined file that contains all the data from the specified range of blocks. The filename of the combined file will be determined by the first block number and the number of blocks included. For example, if you combine traces from block 11000 to block 12000, the resulting combined file will be named `combined_11000_1000.trace`.
    
    You can use the optional arguments to specify the range of blocks to include in the combined file:
    
    - `start-block` (optional): The block number to start with. Any blocks with a number lower than this value will be excluded. If not specified, all blocks from the beginning of the fetched data will be included.
    - `end-block` (optional): The block number to end with. Any blocks with a number higher than this value will be excluded. If not specified, all blocks up to the end of the fetched data will be included.
    
    Additionally, you can specify the output directory for the combined file using the `path` option. If this option is not specified, the file will be placed in the `./data` directory.
    
3. Generating the task file for Authenticated Storage Benchmarks:
        
    ```
    ./target/release/evm-io-tracker seal --input <file-path> --output <dir-path>
    ```
    
    The `seal` command generates two files required for the Authenticated Storage Benchmarks: `real_trace.init` and `real_trace.data`. These files contain the data from the combined file that you specified with the `--input` option.
    
    You can optionally specify the directory path to output the task files with the `--output` option. If this option is not provided, the files will be generated in the default directory `./data`.
    
    After running the `seal` command, copy the `real_trace.init` and `real_trace.data` files to the `./trace` directory in Authenticated Storage Benchmarks. These files will be used as input for the benchmarking process.