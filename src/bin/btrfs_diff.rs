use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
};



#[derive(Debug, PartialEq)]
enum ParsedPath<'a> {
    Raw(&'a str),
    Alloc(String),
}
impl<'a> ParsedPath<'a> {
    fn as_str(&self) -> &str {
        match self {
            ParsedPath::Raw(s) => s,
            ParsedPath::Alloc(s) => s.as_str(),
        }
    }
    fn slice(&self, range: std::ops::RangeFrom<usize>) -> ParsedPath<'a> {
        match self {
            ParsedPath::Raw(s) => ParsedPath::Raw(&s[range]),
            ParsedPath::Alloc(s) => ParsedPath::Alloc(s[range].to_string()),
        }
    }
}

impl<'a> std::fmt::Display for ParsedPath<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParsedPath::Raw(s) => write!(f, "{}", s),
            ParsedPath::Alloc(s) => write!(f, "{}", s),
        }
    }
}

fn split_once_by_unescaped_space<'a>(mut s: &'a str) -> Option<(ParsedPath<'a>, &'a str)> {
    s = s.trim_start_matches(' ');
    if s.is_empty() {
        return None;
    }

    let mut end_index = s.len();
    let mut escaped = false;
    let mut needs_unescaping = false;

    for (i, c) in s.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            needs_unescaping = true;
        } else if c == ' ' {
            end_index = i;
            break;
        }
    }

    let rest = if end_index == s.len() {
        ""
    } else {
        &s[end_index + 1..]
    };

    if !needs_unescaping {
        return Some((ParsedPath::Raw(&s[..end_index]), rest));
    }

    let mut unescaped_path = String::with_capacity(end_index);
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        if i == end_index {
            break;
        }
        if escaped {
            unescaped_path.push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else {
            unescaped_path.push(c);
        }
    }
    Some((ParsedPath::Alloc(unescaped_path), rest))
}

// test
#[test]
fn test_split_once_by_unescaped_space2() {
    assert_eq!(split_once_by_unescaped_space(r""), None);
    assert_eq!(
        split_once_by_unescaped_space(r"a b"),
        Some((ParsedPath::Raw("a"), "b"))
    );
    assert_eq!(
        split_once_by_unescaped_space(r"a\ b"),
        Some((ParsedPath::Alloc("a b".to_string()), ""))
    );
    assert_eq!(
        split_once_by_unescaped_space(r"a\\ b"),
        Some((ParsedPath::Alloc(r"a\".to_string()), "b"))
    );
}

enum BtrfsEventType {
    Mkfile,
    Write,
    UpdateExtent,
    Unlink,
    Rmdir,
    Rename,
    Link,
    Snapshot,
}
struct BtrfsEvent<'a> {
    #[allow(dead_code)]
    event_type: BtrfsEventType,
    path: ParsedPath<'a>,
    dest: Option<ParsedPath<'a>>,
}

fn parse_event_params<'a>(
    rest: &'a str,
    has_dest: bool,
) -> Option<(ParsedPath<'a>, Option<ParsedPath<'a>>)> {
    let (path, rest) = split_once_by_unescaped_space(rest)?;
    if has_dest {
        let (mut dest, _rest) = split_once_by_unescaped_space(rest)?;
        if dest.as_str().starts_with("dest=") {
            dest = dest.slice(5..);
        }
        Some((path, Some(dest)))
    } else {
        Some((path, None))
    }
}

fn parse_event<'a>(line: &'a str) -> Option<BtrfsEvent<'a>> {
    let (cmd, rest) = line.split_once(' ')?;
    match cmd {
        "mkfile" => {
            let (path, _rest) = parse_event_params(rest, false)?;
            Some(BtrfsEvent {
                event_type: BtrfsEventType::Mkfile,
                path,
                dest: None,
            })
        }
        "write" => {
            let (path, _rest) = parse_event_params(rest, false)?;
            Some(BtrfsEvent {
                event_type: BtrfsEventType::Write,
                path,
                dest: None,
            })
        }
        "update_extent" => {
            let (path, _rest) = parse_event_params(rest, false)?;
            Some(BtrfsEvent {
                event_type: BtrfsEventType::UpdateExtent,
                path,
                dest: None,
            })
        }
        "unlink" => {
            let (path, _rest) = parse_event_params(rest, false)?;
            Some(BtrfsEvent {
                event_type: BtrfsEventType::Unlink,
                path,
                dest: None,
            })
        }
        "rmdir" => {
            let (path, _rest) = parse_event_params(rest, false)?;
            Some(BtrfsEvent {
                event_type: BtrfsEventType::Rmdir,
                path,
                dest: None,
            })
        }
        "rename" => {
            let (path, dest) = parse_event_params(rest, true)?;
            Some(BtrfsEvent {
                event_type: BtrfsEventType::Rename,
                path,
                dest,
            })
        }
        "link" => {
            let (path, dest) = parse_event_params(rest, true)?;
            Some(BtrfsEvent {
                event_type: BtrfsEventType::Link,
                path,
                dest,
            })
        }
        "snapshot" => {
            let (path, _rest) = parse_event_params(rest, false)?;
            Some(BtrfsEvent {
                event_type: BtrfsEventType::Snapshot,
                path,
                dest: None,
            })
        }
        _ => None,
    }
}

struct FileState {
    // Was the file newly created. A file can't be created if it was previously modified or removed.
    created: bool,
    // Was the file modified.
    modified: bool,
    // Was the file removed.
    removed: bool,
}
struct EventWriter<W: Write> {
    subvolume_path: String,
    output: W,
    file_states: BTreeMap<String, FileState>,
}

impl<W: Write> EventWriter<W> {
    fn new(subvolume_path: String, output: W) -> Self {
        Self {
            subvolume_path,
            output,
            file_states: BTreeMap::new(),
        }
    }

    fn process_line<'a>(&mut self, line: &'a str) {
        if let Some(event) = parse_event(line) {
            match event.event_type {
                BtrfsEventType::Snapshot => {
                    self.subvolume_path = event.path.as_str().to_string();
                }
                _ => {
                    let p_str = event.path;

                    match event.event_type {
                        BtrfsEventType::Rename | BtrfsEventType::Link => {
                            // Source is destroyed/referenced
                            if matches!(event.event_type, BtrfsEventType::Rename) {
                                self.file_removed(p_str);
                            } else {
                                // Link source must be alive
                                self.file_modified(p_str);
                            }

                            // Dest is created
                            if let Some(dest) = event.dest {
                                let dest = self.fixup(dest);
                                self.file_new(dest);
                            }
                        }
                        BtrfsEventType::Mkfile => {
                            self.file_new(p_str);
                        }
                        BtrfsEventType::Rmdir => {
                            self.file_rmdir(p_str);
                        }
                        BtrfsEventType::Unlink=> {
                            self.file_removed(p_str);
                        }
                        _ => {
                            // Modifications
                            self.file_modified(p_str);
                        }
                    }
                }
            }
        }
    }

    fn fixup<'a>(&self, p: ParsedPath<'a>) -> ParsedPath<'a> {
        let s = p.as_str();
        if s.is_empty() {
            return p;
        }
        if self.subvolume_path.is_empty() {
            return p;
        }
        if s.starts_with(&self.subvolume_path) || s.starts_with(".") {
            p
        } else {
            ParsedPath::Alloc(format!("{}/{}", self.subvolume_path, s))
        }
    }

    fn file_new(&mut self, path: ParsedPath) {
        // Ignore if already present.
        self.file_states.entry(path.to_string()).or_insert(FileState {
                created: true,
                modified: false,
                removed: false,
            });
        }
    fn file_modified(&mut self, path: ParsedPath) {
        match self.file_states.get_mut(path.as_str()) {
            Some(state) => {
                state.modified = true;
            }
            None => {
                self.file_states.insert(path.to_string(), FileState {
                    created: false,
                    modified: true,
                    removed: false,
                });
            }
        }
    }
    fn file_rmdir(&mut self, path: ParsedPath) {
       // Like file_removed, but we need to check for any files within the directory that were created and remove them.
       match self.file_states.get_mut(path.as_str()) {
        Some(state) => {    
            if state.created {
                self.file_states.remove(path.as_str());
            } else {
                state.removed = true;
                return;
            }
        }
        None => {
            self.file_states.insert(path.to_string(), FileState {
                created: false,
                modified: false,
                removed: true,
            });
            return;
        }
       };
       // Remove all files within the directory that were created.
       // we should be able to use lower bound / upper bound
       let prefix = format!("{}/", path.as_str());
       let prefix_end = format!("{}{}", path.as_str(), (('/' as u8) + 1) as char);
       let keys_to_remove = self.file_states.range(prefix..prefix_end).into_iter().map(|(k,_v)| k.to_string()).collect::<Vec<_>>();
       for key in keys_to_remove {
           self.file_states.remove(&key);
       }
    }
    fn file_removed(&mut self, path: ParsedPath) {
        let state = self.file_states.entry(path.to_string()).or_insert(FileState {
                created: false,
                modified: false,
                removed: false,
            });
        // If btrfs recorded the file as created, and then deleted, we can ignore it.
        // Lets us ignore temporary files.
        if state.created {
            self.file_states.remove(path.as_str());
        } else {
            // If the file wasn't created within this receive operation, then it's actually a deletion.
            state.removed = true;
        }
    }

    fn finish(&mut self) {
        let prefix = if self.subvolume_path.is_empty() {
            "".to_string()
        } else {
            format!("{}/", self.subvolume_path)
        };

        for (path, _state) in &self.file_states {
            let mut output_path = path.as_str();
            if !prefix.is_empty() && output_path.starts_with(&prefix) {
                output_path = &output_path[prefix.len()..];
            } else if output_path.starts_with("./") {
                output_path = &output_path[2..];
            }
            // debug_log!("Found modified path: {}", output_path);
            writeln!(self.output, "{}", output_path).unwrap();
        }
    }
}

fn process_buffer<W: Write>(buffer: &mut BufReader<impl Read>, output: &mut W) {
    let mut line = String::new();
    let mut event_writer = EventWriter::new(String::new(), output);
    while buffer.read_line(&mut line).unwrap() > 0 {
        event_writer.process_line(line.trim_end());
        line.clear();
    }
    event_writer.finish();
}

fn is_valid_snapshot_path(path_str: &str) -> bool {
    let path = std::path::Path::new(path_str);
    
    // Reject any explicit relative traversal
    if path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return false;
    }

    let parent_path = path.parent().unwrap_or(std::path::Path::new(""));
    // Canonicalize the parent to ensure they aren't using symlinks to escape
    let canon_parent = std::fs::canonicalize(parent_path).unwrap_or_else(|_| parent_path.to_path_buf());
    
    let grandparent = canon_parent.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str());
    let file_name = path.file_name().and_then(|n| n.to_str());
    
    grandparent == Some(".jj_watchman_snapshots") && file_name.map(|n| n.starts_with("snap_")).unwrap_or(false)
}

use std::os::unix::fs::MetadataExt;

fn find_subvolume_root(start_path: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut current = start_path.to_path_buf();
    loop {
        if let Ok(meta) = std::fs::metadata(&current) {
            if meta.ino() == 256 {
                return Some(current);
            }
        }
        if !current.pop() {
            break;
        }
    }
    None
}

fn print_usage() {
    eprintln!("btrfs_diff - Btrfs snapshot management and differencing tool");
    eprintln!();
    eprintln!("Usage: btrfs_diff <command> [args] [--raw]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  diff <old_snap> <new_snap> [--raw]");
    eprintln!("      Compare two snapshots and output the paths of changed files.");
    eprintln!();
    eprintln!("  snapshot <src> <dest>");
    eprintln!("      Create a read-only snapshot of <src> at <dest>.");
    eprintln!("      Note: <dest> must be inside a .jj_watchman_snapshots directory.");
    eprintln!();
    eprintln!("  delete <path>");
    eprintln!("      Delete the subvolume at <path>.");
    eprintln!("      Note: <path> must be inside a .jj_watchman_snapshots directory.");
    eprintln!();
    eprintln!("  show-root <dir_path>");
    eprintln!("      Print information about the subvolume at <dir_path>.");
    eprintln!();
    eprintln!("  cleanup <dir_path>");
    eprintln!("      Find and delete all orphaned Watchman snapshots below the given directory.");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --raw       Output the raw stream from `btrfs receive --dump` instead of parsed paths.");
    eprintln!("  -h, --help  Print this help message.");
}

fn main() {
    let mut args: Vec<String> = std::env::args().collect();
    let mut raw_output = false;
    args.retain(|s| {
        if s == "--raw" {
            raw_output = true;
            false
        } else {
            true
        }
    });

    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    match args[1].as_str() {
        "help" | "-h" | "--help" => {
            print_usage();
            std::process::exit(0);
        }
        "snapshot" => {
            if args.len() != 4 {
                eprintln!("Error: incorrect number of arguments for 'snapshot'\n");
                print_usage();
                std::process::exit(1);
            }
            if !is_valid_snapshot_path(&args[3]) {
                eprintln!("Error: destination must be inside a .jj_watchman_snapshots directory and start with snap_");
                std::process::exit(1);
            }
            let output = std::process::Command::new("btrfs")
                .arg("subvolume")
                .arg("snapshot")
                .arg("-r")
                .arg(&args[2])
                .arg(&args[3])
                .output()
                .expect("Failed to run btrfs subvolume snapshot");
            
            if !output.status.success() {
                eprintln!("Failed to snapshot: {}", String::from_utf8_lossy(&output.stderr));
                std::process::exit(1);
            }
        }
        "delete" => {
            if args.len() != 3 {
                eprintln!("Error: incorrect number of arguments for 'delete'\n");
                print_usage();
                std::process::exit(1);
            }
            if !is_valid_snapshot_path(&args[2]) {
                eprintln!("Error: path must be inside a .jj_watchman_snapshots directory and start with snap_");
                std::process::exit(1);
            }

            // Subvolumes generated with `snapshot -r` often block deletion on older kernels unless we flip them back to RW
            let _ = std::process::Command::new("btrfs")
                .arg("property")
                .arg("set")
                .arg("-ts")
                .arg(&args[2])
                .arg("ro")
                .arg("false")
                .output();

            let output = std::process::Command::new("btrfs")
                .arg("subvolume")
                .arg("delete")
                .arg(&args[2])
                .output()
                .expect("Failed to run btrfs subvolume delete");
            
            if !output.status.success() {
                eprintln!("Failed to delete subvolume: {}", String::from_utf8_lossy(&output.stderr));
                std::process::exit(1);
            }
        }
        "show-root" => {
            if args.len() != 3 {
                eprintln!("Error: incorrect number of arguments for 'show-root'\n");
                print_usage();
                std::process::exit(1);
            }
            let dir_path = std::path::Path::new(&args[2]);
            let abs_path = std::fs::canonicalize(dir_path).unwrap_or_else(|_| dir_path.to_path_buf());
            if let Some(root) = find_subvolume_root(&abs_path) {
                println!("{}", root.display());
            } else {
                eprintln!("Error: could not find Btrfs subvolume root containing {}", abs_path.display());
                std::process::exit(1);
            }
        }
        "cleanup" => {
            if args.len() != 3 {
                eprintln!("Error: incorrect number of arguments for 'cleanup'\n");
                print_usage();
                std::process::exit(1);
            }
            let base_dir = std::path::Path::new(&args[2]);
            let output = std::process::Command::new("btrfs")
                .arg("subvolume")
                .arg("list")
                .arg("-o")
                .arg(&args[2])
                .output()
                .expect("Failed to run btrfs subvolume list");
            
            if !output.status.success() {
                eprintln!("Failed to list subvolumes: {}", String::from_utf8_lossy(&output.stderr));
                std::process::exit(1);
            }
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if let Some(path_idx) = line.find("path ") {
                    let subvol_path = &line[path_idx + 5..];
                    if subvol_path.contains(".jj_watchman_snapshots") && subvol_path.contains("/snap_") {
                        let mut current = base_dir.to_path_buf();
                        let mut full_path = std::path::PathBuf::new();
                        let mut found = false;
                        
                        // To map the topological btrfs path to our absolute path,
                        // we pop directories off our base_dir until we find a match for subvol_path.
                        // subvol_path is relative to the btrfs subvolume root, so one of the
                        // ancestors of base_dir must be that root!
                        while current.parent().is_some() {
                            let trial = current.join(subvol_path);
                            if trial.exists() {
                                full_path = trial;
                                found = true;
                                break;
                            }
                            current.pop();
                        }
                            
                            if found && is_valid_snapshot_path(&full_path.to_string_lossy()) {
                                println!("Cleaning up old snapshot: {}", full_path.display());
                                let _ = std::process::Command::new("btrfs")
                                    .arg("property").arg("set").arg("-ts").arg(&full_path).arg("ro").arg("false")
                                    .output();
                                let del_out = std::process::Command::new("btrfs")
                                    .arg("subvolume").arg("delete").arg(&full_path)
                                    .output();
                                if let Ok(del) = del_out {
                                    if !del.status.success() {
                                        eprintln!("Failed to delete {}: {}", full_path.display(), String::from_utf8_lossy(&del.stderr));
                                    }
                                }
                            } else if found {
                                eprintln!("Found snapshot but it's not valid: {}", full_path.display());
                            } else {
                                eprintln!("Could not find snapshot for: {}", subvol_path);
                            }
                    }
                }
            }
        }
        "diff" => {
            if args.len() != 4 {
                eprintln!("Error: incorrect number of arguments for 'diff'\n");
                print_usage();
                std::process::exit(1);
            }
            let (dir_path_from, dir_path_to) = (&args[2], &args[3]);

            let send_cmd = std::process::Command::new("btrfs")
                .arg("send")
                .arg("--no-data")
                .arg("--quiet")
                .arg("-p")
                .arg(dir_path_from)
                .arg(dir_path_to)
                .stdout(std::process::Stdio::piped())
                .spawn()
                .expect("Failed to run btrfs send");
            let receive_cmd = std::process::Command::new("btrfs")
                .arg("receive")
                .arg("--quiet")
                .arg("--dump")
                .stdin(send_cmd.stdout.unwrap())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .expect("Failed to run btrfs receive");
            
            let mut reader = std::io::BufReader::new(receive_cmd.stdout.unwrap());
            if raw_output {
                std::io::copy(&mut reader, &mut std::io::stdout()).unwrap();
            } else {
                process_buffer(&mut reader, &mut std::io::stdout());
            }
        }
        unknown => {
            eprintln!("Error: unknown command '{}'\n", unknown);
            print_usage();
            std::process::exit(1);
        }
    }
}

#[test]
fn test_process_buffer() {
    let test_data = r"snapshot        ./work-7                        uuid=4e4137d6-5ff2-ee42-b3e7-4d9694a7da0e transid=30 parent_uuid=01c32218-be8d-e145-aeca-6df71db0ccf2 parent_transid=16
unlink          ./work-7/watchman/CMakeLists.txt
link            ./work-7/watchman/autogen.cmd2  dest=watchman/autogen.cmd
unlink          ./work-7/watchman/autogen.cmd
utimes          ./work-7/watchman/autogen.cmd2  atime=2026-03-21T17:03:47-0700 mtime=2026-03-21T17:03:47-0700 ctime=2026-03-22T10:25:01-0700
rename          ./work-7/watchman/eden          dest=./work-7/o559-13-0
rename          ./work-7/o559-13-0/fs           dest=./work-7/o560-13-0
rmdir           ./work-7/o559-13-0
rename          ./work-7/o560-13-0/config       dest=./work-7/o561-13-0
unlink          ./work-7/o561-13-0/eden_config.thrift
rmdir           ./work-7/o561-13-0
rename          ./work-7/o560-13-0/inodes       dest=./work-7/o563-13-0
rename          ./work-7/o563-13-0/overlay      dest=./work-7/o564-13-0
rmdir           ./work-7/o563-13-0
unlink          ./work-7/o564-13-0/overlay.thrift
rmdir           ./work-7/o564-13-0
rename          ./work-7/o560-13-0/service      dest=./work-7/o566-13-0
unlink          ./work-7/o566-13-0/eden.thrift
unlink          ./work-7/o566-13-0/streamingeden.thrift
rmdir           ./work-7/o566-13-0
rename          ./work-7/o560-13-0/takeover     dest=./work-7/o569-13-0
rmdir           ./work-7/o560-13-0
unlink          ./work-7/o569-13-0/takeover.thrift
rmdir           ./work-7/o569-13-0
unlink          ./work-7/watchman/newfile
mkfile          ./work-7/o1364-20-0
rename          ./work-7/o1364-20-0             dest=./work-7/watchman/CMakeLists.txt
update_extent   ./work-7/watchman/CMakeLists.txt offset=0 len=25468
chown           ./work-7/watchman/CMakeLists.txt gid=1000 uid=1000
chmod           ./work-7/watchman/CMakeLists.txt mode=664
utimes          ./work-7/watchman/CMakeLists.txt atime=2026-03-22T09:58:28-0700 mtime=2026-03-22T09:58:28-0700 ctime=2026-03-22T09:58:28-0700
mkfile          ./work-7/o1366-24-0
rename          ./work-7/o1366-24-0             dest=./work-7/watchman/hi.txt
update_extent   ./work-7/watchman/hi.txt        offset=0 len=12
chown           ./work-7/watchman/hi.txt        gid=1000 uid=1000
chmod           ./work-7/watchman/hi.txt        mode=664
utimes          ./work-7/watchman/hi.txt        atime=2026-03-22T10:01:13-0700 mtime=2026-03-22T10:01:13-0700 ctime=2026-03-22T10:01:13-0700
mkfile          ./work-7/o1367-26-0
rename          ./work-7/o1367-26-0             dest=./work-7/watchman/new\ file\ with\ spaces
chown           ./work-7/watchman/new\ file\ with\ spaces gid=1000 uid=1000
chmod           ./work-7/watchman/new\ file\ with\ spaces mode=664
utimes          ./work-7/watchman/new\ file\ with\ spaces atime=2026-03-22T10:05:00-0700 mtime=2026-03-22T10:05:00-0700 ctime=2026-03-22T10:05:00-0700
utimes          ./work-7/watchman               atime=2026-03-22T10:25:02-0700 mtime=2026-03-22T10:25:01-0700 ctime=2026-03-22T10:25:01-0700
";
    let mut result: Vec<u8> = Vec::new();
    // let lines = test_data.split('\n').collect::<Vec<_>>();
    // let test_data = lines[0..3].join("\n");
    process_buffer(
        &mut std::io::BufReader::new(test_data.as_bytes()),
        &mut result,
    );
    assert_eq!(
        String::from_utf8(result)
            .unwrap()
            .split("\n")
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>(),
        vec![
            "watchman/CMakeLists.txt",
            "watchman/autogen.cmd2",
            "watchman/eden",
            "watchman/hi.txt",
            "watchman/new file with spaces",
            "watchman/newfile",
        ]
    );
}
/*

*/
