extern crate rustc_serialize;
extern crate docopt;
extern crate chrono;
#[macro_use] extern crate hyper;

use docopt::Docopt;
use hyper::Client;
use hyper::client::{response, IntoUrl, RequestBuilder};
use hyper::header::Headers;
use hyper::method::Method;
use hyper::status::StatusCode;
use std::env;
use std::thread;
use std::sync::Arc;

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

    let head_request = match url.into_url() {
        Ok(_u) => client.head(_u),
        Err(_) => {
            println!("{}", "Failed to parse URL");
            std::process::exit(1);
        }
    };

    let resp = match head_request.send() {
        Ok(r) => r,
        Err(_) => {
            println!("{}", "Could not HEAD the given URL");
            std::process::exit(1);
        }
    };

    // let c = client.clone();
    // let path = String::from("/jjw");
    match client.head(&url).send() {
        Ok(resp) => {
            assert_eq!(resp.status, StatusCode::NoContent);
            for item in resp.headers.iter() {
                println!("{}", item);
            }
        }
        Err(s) => println!("{}", s)
    };
    // let thread_action = thread::spawn(move || {
    //
    // });
    //
    // let result = thread_action.join();
    // match result {
    //     Err(_) => println!("All went boom"),
    //     _ => ()
    // }
}
