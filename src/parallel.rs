use hyper::header::{Headers, Range, ContentLength, ByteRangeSpec};
use hyper::Client;

use std::cmp;
use std::collections::LinkedList;
use std::io::{Error, ErrorKind, Read};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::thread;
use std::thread::JoinHandle;

pub struct Download {
    client: Client,
    url: String,
    start: u64,
    size: u32,            // Limit to 4G in a single range request
    bytes_read: u32,      // index of last byte for which a read has been scheduled
    read_size: u32,       // size of individual reads from the socket
    recv: Receiver<()>,   // to allow responsive shutdown
}

impl Download {
    pub fn new(url: String, start: u64, size: u32, rx: Receiver<()>) -> Download {
        let client = Client::new();
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

    #[allow(dead_code)]
    pub fn set_read_size(&mut self, size: u32) {
        self.read_size = size;
    }

    pub fn download(&mut self) -> Result<Vec<u8>, String> {
        // Start performing the actual download
        //
        // Make the request, then read into the local
        // storage buffer. During the download monitor
        // for calls to kill.
        let mut buffer: Vec<u8> = vec![0; self.size as usize];  // Vec::with_capacity((self.size as usize));
        let mut headers = Headers::new();
        let r_start = self.start;
        let r_end = r_start + self.size as u64 - 1;
        headers.set(
            Range::Bytes(
                vec![ByteRangeSpec::FromTo(r_start, r_end)]
            )
        );
        let body = self.client.get(&self.url).headers(headers).send();
        let mut res = match body {
            Ok(res) => res,
            Err(e) => {
                // println!("{:?}", e);
                return Err(String::from("Request failed"))
            }
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
                    // let b_size = cmp::min(self.read_size, self.size - self.bytes_read);
                    // println!("start byte: {:?}, in size: {:?}, trying to read {:?}", b_start, buffer.len(), b_size);
                    let mut range_buffer: &mut [u8] = &mut buffer[b_start as usize .. self.size as usize];
                    let _r_size = match res.read(&mut range_buffer) {
                        Ok(rs) => rs,
                        Err(_) => 0
                    };  // TODO: range might be smaller
                    self.bytes_read += _r_size as u32;
                    if _r_size == 0 && self.bytes_read < self.size {
                        return Err(format!("Got no more bytes after reading {}, but expected {}!", self.bytes_read, self.size))
                    };
                }
            };
        }

        if self.bytes_read < self.size {
            // println!("{:?}", "truncating smaller vec");
            buffer.truncate(self.bytes_read as usize)
        }
        // println!("download of {:?} bytes complete", self.size);
        Ok(buffer)  // TODO: make sure we return a buffer of the right size!
    }
}

pub struct ParallelDownload {
    url: String,
    client: Client,
    downloaders: LinkedList<JoinHandle<Result<Vec<u8>, String>>>,
    downloader_kill_channels: LinkedList<Sender<()>>,
    current_vec: Option<Vec<u8>>,
    chunk_size: u32,
    next_start_byte: u64,
    next_read_offset: u64,
    content_length: u64
}

impl ParallelDownload {
    pub fn new(url: String) -> ParallelDownload {
        let client = Client::new();
        let downloader_list:
            LinkedList<JoinHandle<Result<Vec<u8>, String>>> = LinkedList::new();
        let kill_channel_list: LinkedList<Sender<()>> = LinkedList::new();

        ParallelDownload {
            url: url,
            client: client,
            downloaders: downloader_list,
            downloader_kill_channels: kill_channel_list,
            current_vec: None,
            chunk_size: 50*1024*1024,  // TODO: configure
            next_start_byte: 0,
            next_read_offset: 0,
            content_length: 0
        }
    }

    pub fn kill(&mut self) {
        for k in &self.downloader_kill_channels {
            match k.send(()) {
                // TODO: Handle this properly
                Ok(_) => {},
                Err(_) => {}
            };
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

        // println!("{:?}", "new thread...");

        let next_start_byte: u64 = {
            let mut _b: u64 = start_byte + self.chunk_size as u64;
            if _b > content_length {
                _b = content_length
            };
            _b
        };

        let _u = self.url.clone();
        let _cl = self.content_length;
        let _s = cmp::min(self.chunk_size as u64, _cl - start_byte) as u32;
        let (tx, rx) = channel();
        let mut _d = Download::new(_u, start_byte, _s, rx);
        self.downloader_kill_channels.push_back(tx);
        let _dl_thread = thread::spawn(move || {
            // Start the download, and return the full buffer when complete
            _d.download()
        });
        self.downloaders.push_back(_dl_thread);

        Some(next_start_byte)  // return the next start byte
    }

    pub fn start_download(&mut self) -> Result<(), String> {
        // head to get size and range req support
        let head_resp = try!(match self.client.head(&self.url).send() {
            Ok(r) => Ok(r),
            _ => Err(String::from("Could not HEAD the given URL"))
        });
        // get size from headers
        self.content_length = match head_resp.headers.get() {
            Some(&ContentLength(ref _cl)) => *_cl,  // : & u64
            None => return Err(String::from("No content length found"))
        };
        //println!("{:?}", self.content_length);

        // start filling the thread pools and downloads
        let mut thread_count = 0;
        let mut next_start_byte: u64 = 0;
        let cl = self.content_length;
        let max = 6;
        while (thread_count < max) && (next_start_byte < cl) {
            let start_byte: u64 = next_start_byte;
            next_start_byte = match self.try_start_thread(start_byte, cl) {
                None => cl,
                Some(_u) => {
                    thread_count += 1;
                    _u
                }
            };
        }

        self.next_start_byte = next_start_byte;
        Ok(())
    }
}

impl Read for ParallelDownload {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        // read buf.len() bytes into buf
        // println!("{:?}", self.current_vec);
        match self.current_vec {
            None => {
                // try to get the next buffer
                let dt_handle = match self.downloaders.pop_front() {
                    None => return Ok(0),
                    Some(_dt) => _dt
                };
                let d_result: Vec<u8> = match dt_handle.join() {
                    Err(_e) => {
                        self.kill();
                        //println!("{}", "oh no!");
                        return Err(Error::new(ErrorKind::Other, "oh no!"));
                    },
                    Ok(buf) => {
                        // If we've got a result, it's time to kick off
                        // a new thread (if possible)
                        let cl = self.content_length;
                        let nsb = self.next_start_byte;
                        match self.try_start_thread(nsb, cl) {
                            None => (),
                            Some(nsb) => {
                                self.next_start_byte = nsb
                            }
                        };
                        match buf {
                            Ok(_v) => _v,
                            Err(_es) => {
                                println!("{}", _es);
                                return Err(Error::new(ErrorKind::Other, _es))
                            }
                        }
                    }
                };
                self.current_vec = Some(d_result);
            },
            _ => ()
        };

        let (complete, len) = match self.current_vec {
            Some(ref v) => {
                let nro = self.next_read_offset;
                // println!("{}", v.len());
                let _v_len = v.len() - nro as usize;
                let _len = cmp::min(buf.len(), _v_len);
                let mut buf_sl = &mut buf[.. _len];
                let new_nro = nro + _len as u64;
                let current_buf_sl = &v[nro as usize .. new_nro as usize];
                buf_sl.clone_from_slice(current_buf_sl);

                if new_nro >= v.len() as u64 {
                    // println!("{:?}", "Finished reading buffer");
                    self.next_read_offset = 0;
                    (true, _len)
                } else {
                    self.next_read_offset = new_nro;
                    (false, _len)
                }
            },
            _ => return Err(Error::new(ErrorKind::Other, "...oh no!")),
        };

        if complete {
            self.current_vec = None;
        }

        Ok(len)
    }
}
