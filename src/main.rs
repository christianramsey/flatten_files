use chrono::offset::Utc;
use chrono::DateTime;
use clipboard::ClipboardContext;
use clipboard::ClipboardProvider;
use rayon::prelude::*;
use serde::Deserialize;
use std::env;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use walkdir::WalkDir;

#[derive(Deserialize)]
struct Config {
    general_files: Vec<String>,
    projects: Vec<Project>,
}

#[derive(Deserialize)]
struct Project {
    project_type: String,
    file_types: Vec<String>,
    file_names: Vec<String>,
}

fn read_config_from_str(config_str: &str) -> Result<Config, std::io::Error> {
    let config = serde_json::from_str(config_str)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))?;
    Ok(config)
}

fn read_config() -> Result<Config, std::io::Error> {
    let config_str = fs::read_to_string("config.json")?;
    read_config_from_str(&config_str)
}

fn get_start_dir() -> Result<String, &'static str> {
    let args: Vec<String> = env::args().collect();
    args.get(1)
        .ok_or("Usage: flatten_files <directory>")
        .map(|s| s.to_string())
}

fn create_output_file(start_dir: &str) -> io::Result<(File, PathBuf)> {
    let output_path = PathBuf::from(start_dir).join("flattened_files.txt");
    let output_file = File::create(&output_path)?;
    Ok((output_file, output_path))
}

fn get_entries(start_dir: &str) -> Vec<walkdir::DirEntry> {
    WalkDir::new(start_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .collect::<Vec<_>>()
}

fn get_lines(
    entries: &[walkdir::DirEntry],
    project_type: Option<&str>,
    config: &Config,
) -> Result<Vec<String>, std::io::Error> {
    let (file_types, file_names) = match project_type {
        Some(project_type) => {
            let project_config = config
                .projects
                .iter()
                .find(|project| project.project_type == project_type)
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "Project type not found in configuration",
                    )
                })?;
            (
                project_config.file_types.clone(),
                project_config.file_names.clone(),
            )
        }
        None => {
            let file_types: Vec<String> = config
                .projects
                .iter()
                .flat_map(|project| &project.file_types)
                .cloned()
                .collect();
            let file_names: Vec<String> = config
                .projects
                .iter()
                .flat_map(|project| &project.file_names)
                .cloned()
                .collect();
            (file_types, file_names)
        }
    };

    let mut file_names = [&file_names[..], &config.general_files[..]].concat();
    file_names.extend_from_slice(&config.general_files);

    entries
        .par_iter()
        .filter_map(|entry| {
            let path = entry.path();
            if path.is_file()
                && (file_types.iter().any(|ext| {
                    path.extension()
                        .and_then(OsStr::to_str)
                        .map_or(false, |e| e == ext)
                }) || file_names.iter().any(|name| {
                    path.file_name()
                        .and_then(OsStr::to_str)
                        .map_or(false, |n| n == name)
                }))
            {
                let file = File::open(path).ok()?;
                let reader = BufReader::new(file);
                let metadata = fs::metadata(path).ok()?;
                let modified_time = metadata.modified().ok()?;
                let datetime: DateTime<Utc> = modified_time.into();
                let mut file_lines = vec![format!(
                    "<file name=\"{}\" last_modified=\"{}\">",
                    path.display(),
                    datetime.to_rfc3339()
                )];
                file_lines.extend(reader.lines().filter_map(Result::ok));
                file_lines.push("</file>".to_string());
                Some(Ok(file_lines))
            } else {
                None
            }
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|lines| lines.concat())
}

fn write_lines(output_file: &mut File, lines: &[String]) -> io::Result<()> {
    for line in lines {
        writeln!(output_file, "{}", line)?;
    }
    Ok(())
}

fn copy_to_clipboard(output_path: &PathBuf) -> io::Result<()> {
    let mut ctx: ClipboardContext = ClipboardProvider::new().unwrap();
    let content = fs::read_to_string(output_path)?;
    ctx.set_contents(content).unwrap();
    Ok(())
}

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();
    let project_type = args.get(2).map(String::as_str);

    let start_dir = get_start_dir().expect("Failed to get start directory");
    let (mut output_file, output_path) = create_output_file(&start_dir)?;
    let entries = get_entries(&start_dir);
    let config = read_config()?;
    let lines = get_lines(&entries, project_type, &config)?;
    write_lines(&mut output_file, &lines)?;
    copy_to_clipboard(&output_path)?;
    println!(
        "🎉 Files have been flattened into {}! 🎉",
        output_path.display()
    );

    if env::var("CARGO_PKG_NAME").is_ok() {
        let release_executable_path = env::current_exe()?
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("release");
        if !release_executable_path.exists() {
            println!("Executable not found at {:?}", release_executable_path);
            println!("Please run 'cargo build --release' to build the executable.");
            return Ok(());
        }
        let zshrc_command = format!(
            "echo 'export PATH=\"{}:$PATH\"' >> ~/.zshrc",
            release_executable_path.display()
        );
        println!("Add the following command to your ~/.zshrc file:");
        println!("{}", zshrc_command);
    }

    Ok(())
}
