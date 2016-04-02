use hyper::header::{Headers, Range, ContentLength, ByteRangeSpec};
use hyper::Client;

use std::cmp;
use std::collections::LinkedList;
use std::io::{Error, ErrorKind, Read};
use std::sync::Arc;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::thread;
use std::thread::JoinHandle;

pub struct Download {
    client: Arc<Client>,
    url: String,
    start: u64,
    size: u32,            // Limit to 4G in a single range request
    bytes_read: u32,      // index of last byte for which a read has been scheduled
    read_size: u32,       // size of individual reads from the socket
    recv: Receiver<()>,   // to allow responsive shutdown
}

impl Download {
    pub fn new(client: Arc<Client>, url: String,
               start: u64, size: u32, rx: Receiver<()>) -> Download {
        Download {
            client: client,
            url: url,
            start: start,
            size: size,
            bytes_read: 0,
            read_size: 1024*1024,
            recv: rx
        }
    }

    pub fn set_read_size(&mut self, size: u32) {
        self.read_size = size;
    }

    pub fn download(&mut self) -> Result<Vec<u8>, String> {
        // Start performing the actual download
        //
        // Make the request, then read into the local
        // storage buffer. During the download monitor
        // for calls to kill.
        let mut buffer: Vec<u8> = Vec::with_capacity((self.size as usize));
        let mut headers = Headers::new();
        let r_start = self.start;
        let r_end = r_start + (self.size as u64) - 1;
        headers.set(
            Range::Bytes(
                vec![ByteRangeSpec::FromTo(r_start, r_end)]
            )
        );
        let body = self.client.get(&self.url).headers(headers).send();
        let mut res = match body {
            Ok(res) => res,
            _ => return Err(String::from("Request failed"))
        };

        // start reading the request, with checks for abort
        while self.bytes_read < self.size {
            match self.recv.try_recv() {
                Ok(_) | Err(TryRecvError::Disconnected) => {
                    // Any communication means stop
                    // TODO: close connection?
                    break;
                }
                Err(TryRecvError::Empty) => {
                    // The actual work goes here
                    let b_start = self.bytes_read;
                    let b_end = b_start + self.read_size - 1;
                    let mut range_buffer: &mut [u8] = &mut buffer[b_start as usize .. b_end as usize];
                    res.read(&mut range_buffer);  // TODO: range might be smaller
                    self.bytes_read += self.read_size;
                }
            };
        }

        Ok(buffer)
    }
}

pub struct ParallelDownload {
    url: String,
    client: Arc<Client>,
    downloaders: LinkedList<JoinHandle<Result<Vec<u8>, String>>>,
    downloader_kill_channels: LinkedList<Sender<()>>,
    current_vec: Option<Vec<u8>>,
    chunk_size: u32,
    next_start_byte: u64,
    content_length: u64
}

impl ParallelDownload {
    pub fn new(url: String) -> ParallelDownload {
        let client = Arc::new(Client::new());
        let downloader_list:
            LinkedList<JoinHandle<Result<Vec<u8>, String>>> = LinkedList::new();
        let kill_channel_list: LinkedList<Sender<()>> = LinkedList::new();

        ParallelDownload {
            url: url,
            client: client,
            downloaders: downloader_list,
            downloader_kill_channels: kill_channel_list,
            current_vec: None,
            chunk_size: 20*1024*1024,  // TODO: configure
            next_start_byte: 0,
            content_length: 0
        }
    }

    pub fn kill(&mut self) {
        for k in &self.downloader_kill_channels {
            k.send(());
        }
    }

    fn try_start_thread(&mut self, start_byte: u64, content_length: u64)
            -> Option<u64> {
        // Attempt to create a new download thread for a given
        // start byte.
        //
        // Some(end_byte) indicates that a thread has been created
        // None indicates that the content_length has been reached
        if start_byte >= content_length {
            return None;
        }

        let end_byte: u64 = {
            let mut _b: u64 = start_byte + self.chunk_size as u64;
            if _b > content_length {
                _b = content_length
            };
            _b
        };

        let _c = self.client.clone();
        let _u = self.url.clone();
        let _s = self.chunk_size.clone();
        let (tx, rx) = channel();
        let mut _d = Download::new(_c, _u, start_byte, _s, rx);
        self.downloader_kill_channels.push_back(tx);
        let _dl_thread = thread::spawn(move || {
            // Start the download, and return the full buffer when complete
            _d.download()
        });
        self.downloaders.push_back(_dl_thread);

        Some(end_byte)  // return the new end_byte
    }

    pub fn start_download(&mut self) -> Result<(), String> {
        // head to get size and range req support
        //let head_request = try!(match Url::from_str(&self.url) {
        //    Ok(_u) => Ok(self.client.head(_u)),
        //    Err(s) => Err(String::from("Failed to parse URL"))
        //});
        let head_resp = try!(match self.client.head(&self.url).send() {
            Ok(r) => Ok(r),
            _ => Err(String::from("Could not HEAD the given URL"))
        });
        // get size from headers
        self.content_length = match head_resp.headers.get() {
            Some(&ContentLength(ref _cl)) => *_cl,  // : & u64
            None => return Err(String::from("No content length found"))
        };

        // start filling the thread pools and downloads
        let mut thread_count = 0;
        let mut end_byte: u64 = 0;
        let cl = self.content_length;
        let max = 10;
        while (thread_count < max) && (end_byte < cl) {
            let start_byte: u64 = end_byte;
            end_byte = match self.try_start_thread(start_byte, cl) {
                None => self.content_length,
                Some(_u) => {
                    thread_count += 1;
                    _u
                }
            };
        }

        self.next_start_byte = end_byte;
        Ok(())
    }

    fn r(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        // read buf.len() bytes into buf
        match self.current_vec.take() {
            None => {
                // try to get the next buffer
                let dt_handle = match self.downloaders.pop_front() {
                    None => return Ok(0),
                    Some(_dt) => _dt
                };
                let d_result: Vec<u8> = match dt_handle.join() {
                    Err(_e) => {
                        self.kill();
                        return Err(Error::new(ErrorKind::Other, "oh no!"));
                    },
                    Ok(buf) => {
                        // If we've got a result, it's time to kick off
                        // a new thread (if possible)
                        let cl = self.content_length;
                        let nsb = self.next_start_byte;
                        let _nsb = self.try_start_thread(nsb, cl);
                        match _nsb {
                            None => (),
                            Some(nsb) => {
                                self.next_start_byte = nsb
                            }
                        };
                        match buf {
                            Ok(_b) => _b,
                            Err(_es) => return Err(Error::new(ErrorKind::Other, _es))
                        }
                    }
                };
                self.current_vec = Some(d_result);
            },
            _ => ()
        };

        match self.current_vec.take() {
            Some(v) => {
                let nsb = self.next_start_byte;
                let _v_len = v.len() - nsb as usize;
                let _len = cmp::min(buf.len(), _v_len);
                let mut buf_sl = &mut buf[.. _len];
                let new_nsb = _len as u64 + nsb;
                let current_buf_sl = &v[nsb as usize .. new_nsb as usize];
                buf_sl.clone_from_slice(current_buf_sl);

                if new_nsb == v.len() as u64 {
                    self.current_vec = None;
                    self.next_start_byte = 0;
                } else {
                    self.next_start_byte = new_nsb;
                };

                return Ok(_len)
            },
            _ => return Err(Error::new(ErrorKind::Other, "oh no!")),
        };
    }
}

impl Read for ParallelDownload {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        // read buf.len() bytes into buf
        self.r(buf)
    }
}
