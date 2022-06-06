use log::{set_logger_raw, Log, LogLevelFilter, LogMetadata, LogRecord};

struct Logger {
    // file: Mutex<File>,
}

impl Logger {
    fn new() -> Logger {
        // let mut file = File::create("/home/user/log.txt").unwrap(); 
        Logger {
            // file: Mutex::new(file),
        }
    }
}

impl Log for Logger {
    fn enabled(&self, _: &LogMetadata) -> bool {
        true
    }

    fn log(&self, record: &LogRecord) {
        println!("{}: {}", record.target(), record.args());

        // writeln!(&mut self.file.lock().unwrap(), "{}: {}", record.target(), record.args());
    }
}

pub fn init_logger() {
    unsafe {
        set_logger_raw(|max_log_level| {
            max_log_level.set(LogLevelFilter::Trace);
            &Logger::new()
        }).expect("Can't initialize logger");
    }
}
