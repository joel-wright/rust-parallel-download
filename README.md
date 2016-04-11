# rust-parallel-download

```
    Usage: parallel-url-download <url> [options]
       parallel-url-download (-h | --help)
       parallel-url-download (-v | --version)
    Options:
      -o, --out=<filename>     The filename to save the URL contents into
      -t, --threads=<threads>  Number of download threads to use [default: 4]
      -c, --chunksize=<size>   Buffer size for each download thread (in MB) [default: 25]
      -h, --help               display this help and exit
      -v, --version            output version information and exit
```

Learning rust by implementing a basic parallel URL downloader.
