extern crate rustc_serialize;
extern crate docopt;
extern crate chrono;
#[macro_use] extern crate hyper;

use docopt::Docopt;
use hyper::Client;
use hyper::status::StatusCode;

use std::env;
use std::io::Read;
use std::sync::Arc;

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

    let client = Arc::new(Client::new());

    match client.head(&url).send() {
        Ok(resp) => {
            assert_eq!(resp.status, StatusCode::NoContent);
            for item in resp.headers.iter() {
                println!("{}", item);
            }
        }
        Err(s) => println!("{}", s)
    };

    let mut download = ParallelDownload::new(url.clone());
    let sd = download.start_download();
    match sd {
        Err(s) => {
            println!("{}", s);
            std::process::exit(1);
        }
        _ => {
            // Simple attempt to use the download
            let mut b = [0,0,0,0];
            let nr = download.read(&mut b);
            match nr {
                Err(_) => println!("{}", "Failed to download"),
                Ok(n) => println!("{} bytes read", n)
            }

            // TODO: Download URL and write to 'out'
        }
    }
}
