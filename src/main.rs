extern crate rustc_serialize;
extern crate docopt;
extern crate chrono;
#[macro_use] extern crate hyper;

use docopt::Docopt;

//use std::env;
use std::fs::File;
use std::io::{Read, Write};

mod parallel;
use parallel::ParallelDownload;

const VERSION: &'static str = "Parallel URL Download v0.1.0";
const USAGE: &'static str = "
Usage: parallel-url-download <url> [options]
       parallel-url-download (-h | --help)
       parallel-url-download (-v | --version)

Options:
    -o, --out=<filename>     The filename to save the URL contents into
    -t, --threads=<threads>  Number of download threads to use [default: 4]
    -c, --chunksize=<size>   Buffer size for each download thread (in MB) [default: 25]
    -h, --help               display this help and exit
    -v, --version            output version information and exit
";

#[derive(Debug, RustcDecodable)]
struct Args {
    arg_url: String,
    flag_out: String,
    flag_threads: u32,
    flag_chunksize: u32,
    flag_help: bool,
    flag_version: bool,
}

fn main() {
    let args: Args = Docopt::new(USAGE)
                            .and_then(|dopt| dopt.version(Some(String::from(VERSION)))
                                                 .decode())
                            .unwrap_or_else(|e| e.exit());

    let url = args.arg_url;
    let mut out = args.flag_out;
    let chunk_size = args.flag_chunksize;
    let threads = args.flag_threads;

    if out == "" {
        let split_url = url.split("/");
        let mut url_vec: Vec<&str> = split_url.collect();
        let filename = url_vec.pop();
        out = match filename {
            Some(_fn) => String::from(_fn),
            None => {
                println!("No output filename could be derived from the given URL.");
                std::process::exit(1);
            }
        };
        //out = String::from(url_vec.pop());
    }

    println!("Downloading {} to: {}", url, out);

    let mut download = ParallelDownload::new(url.clone(), chunk_size, threads);
    let sd = download.start_download();
    match sd {
        Err(e) => {
            println!("Failed to start download: {}", e);
            std::process::exit(1);
        }
        _ => {
            let mut f = match File::create(out) {
                Err(_) => {
                    println!("Failed to create output file");
                    std::process::exit(1);
                },
                Ok(_f) => _f
            };

            // Read from the parallel downloader as if it's a single stream
            let mut finished = false;
            while !finished {
                let mut buffer: [u8; 1024*1024] = [0; 1024*1024];
                let _read_size = match download.read(&mut buffer) {
                    Ok(_s) => _s,
                    Err(e) => {
                        println!("Download Failed: {}", e);
                        download.kill();
                        std::process::exit(1);
                    }
                };
                if _read_size > 0 {
                    match f.write_all(&buffer[.. _read_size]) {
                        Err(e) => {
                            println!("Failed to write output: {}", e);
                            download.kill();
                            std::process::exit(1);
                        },
                        _ => {}
                    }
                } else {
                    finished = true;
                    println!("Download Complete");
                };
            }
        }
    }
}
