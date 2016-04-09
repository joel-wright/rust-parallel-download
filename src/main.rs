extern crate rustc_serialize;
extern crate docopt;
extern crate chrono;
#[macro_use] extern crate hyper;

use docopt::Docopt;

use std::env;
use std::fs::File;
use std::io::{Read, Write};

mod parallel;
use parallel::ParallelDownload;

const USAGE: &'static str = "
Usage: swift [options] [<command>]

Options:
    -u, --url=<url>          URL to download
    -o, --output=<output>    Output path for the download
    -h, --help               display this help and exit
    -v, --version            output version information and exit
";

#[derive(Debug, RustcDecodable)]
struct Args {
    arg_command: Option<String>,
    flag_url: Option<String>,
    flag_output: Option<String>,
}

fn get_arg(arg: Option<String>, os_var: String) -> Option<String> {
    match env::var(os_var) {
        Ok(v) => match arg {
            Some(u) => Some(u),
            None => Some(v)
        },
        Err(_) => match arg {
            Some(u) => Some(u),
            None => None
        }
    }
}

fn main() {
    let args: Args = Docopt::new(USAGE)
                            .and_then(|dopt| dopt.decode())
                            .unwrap_or_else(|e| e.exit());

    let url = get_arg(args.flag_url, String::from("RP_URL")).unwrap();
    let out = get_arg(args.flag_output, String::from("RP_OUT")).unwrap();
    println!("{}", out);

    let mut download = ParallelDownload::new(url.clone());
    let sd = download.start_download();
    match sd {
        Err(s) => {
            println!("{}", s);
            std::process::exit(1);
        }
        _ => {
            let mut f = match File::create(out) {
                Err(_) => {
                    println!("{}", "Failed to create output file");
                    std::process::exit(1);
                },
                Ok(_f) => _f
            };

            // TODO: Download URL and write to 'out'
            let mut finished = false;
            while !finished {
                let mut buffer: [u8; 1024*1024] = [0; 1024*1024];
                let _read_size = match download.read(&mut buffer) {
                    Ok(_s) => _s,
                    Err(_) => {
                        println!("{}", "Download failed");
                        download.kill();
                        std::process::exit(1);
                    }
                };
                if _read_size > 0 {
                    match f.write_all(&buffer[.. _read_size]) {
                        Err(_) => {
                            println!("{}", "Failed to write output");
                            download.kill();
                            std::process::exit(1);
                        },
                        _ => {
                            println!("{}", "Saved");
                        }
                    }
                } else {
                    finished = true;
                };
            }
        }
    }
}
