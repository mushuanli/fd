use std::io;
use std::io::Write;
use std::path::Path;

use argmax::Command;

use crate::error::print_error;
use crate::exit_codes::ExitCode;

struct Outputs {
    header: Option<String>,  // 新增：存储 header
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}
pub struct OutputBuffer {
    null_separator: bool,
    outputs: Vec<Outputs>,
}

impl OutputBuffer {
    pub fn new(null_separator: bool) -> Self {
        Self {
            null_separator,
            outputs: Vec::new(),
        }
    }

    fn push(&mut self, header: Option<String>, stdout: Vec<u8>, stderr: Vec<u8>) {
        self.outputs.push(Outputs { header, stdout, stderr });
    }

    fn write(self) {
        // Avoid taking the lock if there is nothing to do.
        // If null_separator is true, then we still need to write the
        // null separator, because the output may have been written directly
        // to stdout
        if self.outputs.is_empty() && !self.null_separator {
            return;
        }

        let stdout = io::stdout();
        let stderr = io::stderr();

        // While we hold these locks, only this thread will be able
        // to write its outputs.
        let mut stdout = stdout.lock();
        let mut stderr = stderr.lock();

        for output in self.outputs.iter() {
            // 先输出 header
            if let Some(ref header) = output.header {
                let _ = writeln!(stdout, "{}", header);
            }
            let _ = stdout.write_all(&output.stdout);
            let _ = stderr.write_all(&output.stderr);
        }
        if self.null_separator {
            // If null_separator is enabled, then we should write a \0 at the end
            // of the output for this entry
            let _ = stdout.write_all(b"\0");
        }
    }
}

/// 生成执行头信息字符串
pub fn format_exec_header(cmd: &Command, path: &Path) -> String {
    let cmd_name = cmd.get_program().to_string_lossy();
    let path_str = path.to_string_lossy();
    format!("\n==={} {}===", cmd_name, path_str)
}

/// Executes a command.
pub fn execute_commands<I: Iterator<Item = io::Result<Command>>>(
    cmds: I,
    mut output_buffer: OutputBuffer,
    enable_output_buffering: bool,
    header: Option<String>,  // 新增参数
) -> ExitCode {
    let mut first = true;
    
    for result in cmds {
        let mut cmd = match result {
            Ok(cmd) => cmd,
            Err(e) => return handle_cmd_error(None, e),
        };

        // Spawn the supplied command.
        let output = if enable_output_buffering {
            cmd.output()
        } else {
            // If running on only one thread, don't buffer output
            // Allows for viewing and interacting with intermediate command output
            
            // 非缓冲模式下，直接打印 header
            if first {
                if let Some(ref h) = header {
                    let stdout = io::stdout();
                    let mut handle = stdout.lock();
                    let _ = writeln!(handle, "{}", h);
                    let _ = handle.flush();
                }
                first = false;
            }
            
            cmd.spawn().and_then(|c| c.wait_with_output())
        };

        // Then wait for the command to exit, if it was spawned.
        match output {
            Ok(output) => {
                if enable_output_buffering {
                    // 缓冲模式下，将 header 与输出一起存储
                    let h = if first {
                        first = false;
                        header.clone()
                    } else {
                        None
                    };
                    output_buffer.push(h, output.stdout, output.stderr);
                }
                if output.status.code() != Some(0) {
                    output_buffer.write();
                    return ExitCode::GeneralError;
                }
            }
            Err(why) => {
                output_buffer.write();
                return handle_cmd_error(Some(&cmd), why);
            }
        }
    }
    output_buffer.write();
    ExitCode::Success
}

pub fn handle_cmd_error(cmd: Option<&Command>, err: io::Error) -> ExitCode {
    match (cmd, err) {
        (Some(cmd), err) if err.kind() == io::ErrorKind::NotFound => {
            print_error(format!(
                "Command not found: {}",
                cmd.get_program().to_string_lossy()
            ));
            ExitCode::GeneralError
        }
        (_, err) => {
            print_error(format!("Problem while executing command: {err}"));
            ExitCode::GeneralError
        }
    }
}
