use hyper::header::{Headers, Range, ContentLength};
use hyper::method::Method;
//use hyper::client::request;
use hyper::client::response;
use hyper::Url;

use std::cmp;
use std::collections::LinkedList;
use std::io::Read;
use std::sync::mpsc::channel;
use std::thread;

pub struct Download {
    client: Client,
    url: String,
    start: u64,
    size: u32,            // Limit to 4G in a single range request

    buffer: Vec<u8>,
    bytes_read: u32,      // index of last byte for which a read has been scheduled
    read_size: mut u32,   // size of individual reads from the socket
    kill: Sender,         // Sender and receiver are used internally
    recv: Receiver,       // to allow responsive shutdown
}

impl Download {
    pub fn new(client: Client, url: String,
               start: u64, size: u32) -> Download {
        let (tx, rx) = channel();
        let b: Vec<u8> = Vec::with_capacity((size as usize));

        Download {
            client: Client,
            url: url,
            start: start,
            size: size,
            buffer: b,
            bytes_read: 0,
            read_size: 1024**2,
            send: tx,
            recv: rx
        }
    }

    pub fn set_read_size(&mut self, size: u32) {
        self.read_size = size;
    }

    pub fn kill(&mut self) {
        self.send.send(());
    }

    pub fn download(&mut self) {
        /// Start performing the actual download
        ///
        /// Make the request, then read into the local
        /// storage buffer. During the download monitor
        /// for calls to kill.

        let mut headers = Headers::new();
        let r_start = self.start;
        let r_end = r_start + (self.size as u64) - 1;
        headers.set(
            Range::Bytes(
                vec![ByteRangeSpec::FromTo(r_start, r_end)
            )
        );
        let body = self.client.get(self.url).headers(headers).send();
        let res = match body {
            Ok(res) => res,
            _ => return Err(String::from("Request failed"))
        };

        // start reading the request, with checks for abort
        while (self.bytes_read < self.size) {
            match self.recv.try_recv() {
                Ok(_) | Err(comm::Disconnected) => {
                    // Any communication means stop
                    // TODO: close connection?
                    break;
                }
                Err(comm::Empty) => {
                    // The actual work goes here
                    let b_start = self.bytes_read;
                    let b_end = b_start + self.chunk_size - 1;
                    let mut range_buffer = &self.buffer[b_start..b_end];
                    res.read(&mut b);
                    self.bytes_read += self.read_size;
                }
            };
        }

        self.b
    }
}

pub struct ParallelDownload {
    url: String,
    client: Client,
    downloaders: LinkedList<Thread>,
    current_buf: Option<&Vec<u8>>,
    chunk_size: u64
}

impl ParallelDownload<String> {
    pub fn new(url: String) -> ParallelDownload<String> {
        let client = Client::new();
        let downloader_list = LinkedList<Download>::new();

        ParallelDownload {
            url: url,
            client: client,
            downloaders: downloader_list,
            current_buf: None,
            chunk_size: 20*1024**2  // TODO: configure
        }
    }

    pub fn kill(&mut self) {
        for d in self.downloaders {
            d.kill();
        }
    }

    fn try_start_thread(&mut self, start_byte: u64, content_length: u64)
            -> Option<u64> {
        /// Attempt to create a new download thread for a given
        /// start byte.
        ///
        /// Some(end_byte) indicates that a thread has been created
        /// None indicates that the content_length has been reached
        if start_byte >= content_length {
            return None;
        }

        let end_byte: u64 = {
            let _b: u64 = start_byte + self.chunk_size;
            if _b > content_length {
                _b = content_length
            };
            _b
        }

        let _c = self.client.clone();
        let _u = self.url.clone();
        let _s = self.chunk_size.clone()
        let _d = Download(_c, _u, start_byte, _s);
        let _dl_thread = thread::spawn(|_d| {
            // Start the download, and return the full buffer when complete
            _d.download()
        });
        self.downloaders.push_back(_dl_thread);

        Some(end_byte)  // return the new end_byte
    }

    fn start_download(&mut self) {
        // head to get size and range req support
        let head_request = try!(match self.url.into_url() {
            Ok(_u) => Ok(self.client.head(_u))
            Err(s) => Err(String::from("Failed to parse URL"))
        });
        let head_resp = try!(match head_request.send() {
            Ok(r) => Ok(r),
            _ => Err(String::from("Could not HEAD the given URL"))
        });
        // get size from headers
        let content_length = try!(match head_resp.headers.get(ContentLength) {
            Some(_cl) => _cl.deref()  // : & u64
            None => Err(String::from("No content length found"))
        });

        // start filling the thread pools and downloads
        let thread_count = 0;
        let end_byte: u64 = 0;
        while thread_count < max and end_byte < content_length {
            let start_byte: u64 = end_byte;
            end_byte = match self.try_start_thread(start_byte, content_length) {
                None => content_length;
                Some(_u) => {
                    thread_count += 1;
                    _u
                }
            };
        }
    }
}

impl Read for ParallelDownload {
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        // read buf.len() bytes into buf
        let current_buf = match self.current_buf {
            None => {
                // try to get the next buffer
                let dt_handle = match self.downloaders.pop_front() {
                    None => return Ok(0),
                    Some(_dt) => _dt
                };
                let d_result = match dt_handle.join() {
                    Err(_e) => {
                        self.kill();
                        return Err(_e);
                    },
                    Ok(buf) => buf
                };
            },
            Some(_b) => _b
        };

        let _ln = cmp::min(buf.len(), current_buf.len());
        buf_sl = &buf[.. _ln];
        current_buf_sl = &current_buf[.. _ln];
        buf_sl.clone_from_slice(current_buf_sl);

        if _ln == buf.len {
            self.current_buf = None;
        } else {
            self.current_buf = Some(&current_buf[_ln ..]);
        }

        Ok(_ln)
    }
}
