use std::{
    env,
    fmt::Write as _,
    io::{self, BufRead, BufReader, BufWriter, Write as _},
    path::{Path, PathBuf},
    process::{ChildStdout, Command, Stdio},
    sync::mpsc,
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use fs_err as fs;

trait ReadExt: io::Read {
    fn read_into_string(&mut self) -> io::Result<String> {
        let mut buf = String::new();
        self.read_to_string(&mut buf).map(|_| buf)
    }
}

impl<T: io::Read> ReadExt for T {}

fn spawn_child(command_args: &[&str]) -> ChildStdout {
    let mut command = Command::new(&command_args[0]);

    for arg in command_args.iter().skip(1) {
        command.arg(arg);
    }

    command
        .stdout(Stdio::piped())
        .spawn()
        .expect("Unable to spawn program")
        .stdout
        .unwrap()
}

fn get_device_numbers(device_name: &str) -> Vec<u8> {
    let mut child_stdout = spawn_child(&["xinput", "list"]);
    let output = child_stdout.read_into_string().unwrap();
    let (device_name, output) = (device_name.to_lowercase(), output.to_lowercase());

    let matched_lines = output.lines().filter(|line| line.contains(&device_name));

    parse_device_numbers(matched_lines)
}

fn parse_device_numbers<'a, I>(command_output: I) -> Vec<u8>
where
    I: Iterator<Item = &'a str>,
{
    let mut numbers = vec![];

    for line in command_output.filter(|line| line.contains("id=")) {
        let id_position = line.rfind("id=").unwrap() + 3;
        let number = line[id_position..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>();
        let number = number.parse::<u8>().expect("Failed to parse id");
        numbers.push(number);
    }

    numbers
}

fn turn_on_listeners(device_numbers: &[u8]) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();

    for &number in device_numbers {
        let tx = tx.clone();

        thread::spawn(move || {
            activate_number_listener(tx.clone(), number);
        });
    }

    rx
}

fn activate_number_listener(tx: mpsc::Sender<String>, number: u8) {
    let child_stdout = spawn_child(&["xinput", "test", &number.to_string()]);
    let mut reader = BufReader::new(child_stdout);

    let mut line = String::new();

    loop {
        let micros_since_the_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_micros();

        write!(line, "{} ", micros_since_the_epoch).unwrap();

        reader.read_line(&mut line).unwrap();
        if line.is_empty() {
            break;
        }
        tx.send(line.clone()).unwrap();
        line.clear();
    }
}

fn write_uncompressed(file: fs::File, receiver: mpsc::Receiver<String>) {
    let mut writer = BufWriter::with_capacity(4096, file);

    while let Ok(line) = receiver.recv() {
        print!("{}", line);
        write!(writer, "{}", line).expect("Failed to write to file");
    }

    writer.flush().expect("Failed to flush file");
}

fn get_log_file_path(data_dir: &Path) -> PathBuf {
    let file_count = fs::read_dir(&data_dir)
        .expect("Could not read data directory")
        .count();

    let log_file_suffix = format!("tila-{file_count}.log");
    data_dir.join(log_file_suffix)
}

fn create_new_log_file() -> fs::File {
    let data_dir = dirs::data_dir()
        .expect("Could not get data directory")
        .join("tila");

    create_folder_if_not_existent(&data_dir);
    let path = get_log_file_path(&data_dir);
    fs::File::create(path).expect("Could not create log file")
}

fn create_folder_if_not_existent(path: &Path) {
    if !Path::new(path).exists() {
        fs::create_dir(path).expect(&format!("Could not create directory at {}", path.display()))
    }
}

fn run_listeners() {
    let device_numbers = dbg!(get_device_numbers("keychron"));

    let receiver = turn_on_listeners(&device_numbers);

    let log_file = create_new_log_file();

    write_uncompressed(log_file, receiver);
}

fn main() {
    let mut args = env::args().skip(1).collect::<Vec<_>>();

    if args.is_empty() {
        run_listeners();
    } else {
        decode(args.pop().unwrap());
    }
}

fn decode(path: impl AsRef<Path>) {
    let contents = fs::read_to_string(path.as_ref()).expect("could not read file");

    let keycode_translation = sugars::hmap! {
        24 => 'q',
        25 => 'w',
        26 => 'e',
        27 => 'r',
        28 => 't',
        29 => 'y',
        30 => 'u',
        31 => 'i',
        32 => 'o',
        33 => 'p',
        38 => 'a',
        39 => 's',
        40 => 'd',
        41 => 'f',
        42 => 'g',
        43 => 'h',
        44 => 'j',
        45 => 'k',
        46 => 'l',
        52 => 'z',
        53 => 'x',
        54 => 'c',
        55 => 'v',
        56 => 'b',
        57 => 'n',
        58 => 'm',
        65 => ' ',
    };

    let mut results = String::new();

    for line in contents.lines().map(|line| line.trim()) {
        // 1652024669524708 key release 36
        let mut split_iter = line.split_whitespace();

        let _timestamp = split_iter.next().unwrap(); // 1652024669524708
        let _key_keyword = split_iter.next().unwrap(); // key
        let operation = split_iter.next().unwrap(); // "press" or "release"
        let keycode = split_iter.next().unwrap(); // 36

        if operation == "press" {
            let keycode = keycode.parse::<u8>().expect("Could not parse keycode");
            if let Some(ch) = keycode_translation.get(&keycode) {
                results.push(*ch);
            }
        }
    }

    println!("{results}");
}
