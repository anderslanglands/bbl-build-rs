use std::{
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
};

#[cfg(target_os = "linux")]
use regex::Regex;

#[cfg(target_os = "windows")]
fn print_link_args(dst: &Path, project_name: &str) {
    // get linker args from build.ninja
    let contents = read_link_line(dst, project_name);

    println!("cargo:rustc-link-search=native={}/build", dst.display());

    for token in contents.split_whitespace() {
        let path = Path::new(&token.replace('\\', "/")).to_owned();
        if let Some(parent) = path.parent() {
            if !parent.to_string_lossy().is_empty() {
                println!("cargo:rustc-link-search=native={}", parent.display());
            }
        }
        println!(
            "cargo:rustc-link-lib={}",
            path.file_stem().unwrap().to_string_lossy()
        );
    }
}

#[cfg(target_os = "linux")]
fn print_link_args(dst: &Path, project_name: &str) {
    // get linker args from build.ninja
    let contents = read_link_line(dst, project_name);

    println!("cargo:rustc-link-search=native={}/build", dst.display());

    for token in contents.split_whitespace() {
        let path = Path::new(&token.replace('\\', "/")).to_owned();
        if let Some(parent) = path.parent() {
            if !parent.to_string_lossy().is_empty() {
                println!("cargo:rustc-link-search=native={}", parent.display());
            }
        }

        // stem will be either "libsomething.a/.so.2.1" or "-lstdc++""
        let stem = path.file_stem().unwrap().to_string_lossy();

        let lib = if let Some(stripped) = stem.strip_prefix("-l") {
            stripped.to_string()
        } else if let Some(stripped) = stem.strip_prefix("lib") {
            if let Some(stripped) = stripped.strip_suffix(".a") {
                stripped.to_string()
            } else {
                let re = Regex::new(r"(.*)\.so[\.0-9]*|\.a").unwrap();
                if let Some(caps) = re.captures(stripped) {
                    caps[1].to_string()
                } else {
                    stripped.to_string()
                }
            }
        } else {
            panic!("unknown lib form \"{stem}\" from \"{}\"", path.display());
        };

        println!("cargo:rustc-link-lib={lib}");
    }
}

fn read_link_line(dst: &Path, project_name: &str) -> String {
    let build_ninja = format!("{}/build/build.ninja", dst.display());
    let contents = std::fs::read_to_string(&build_ninja)
        .unwrap_or_else(|e| panic!("could not read {build_ninja}: {e}"));

    // now parse the build.ninja to find the link line in there
    // this could very well be the world's worst parser
    let index = contents
        .find(&format!(
            "build {}-link-libraries.txt: ECHO_EXECUTABLE_LINKER",
            project_name
        ))
        .unwrap_or_else(|| panic!("could not find echo target in {}", build_ninja));
    let contents = &contents[index..];

    // link args are contained between | and ||
    let index = contents.find("| ")
        .unwrap_or_else(|| panic!("could not find beginning of linker args in {}", build_ninja));

    let contents = &contents[index+2..];

    let end = contents.find("||").unwrap_or_else(||panic!("could not find end of linker args in {}", build_ninja));
    // on windows drive letters are encoded as C$:\
    let contents = contents[..end].replace("$:", ":");

    contents.to_string()
}

pub struct Config {
    project_name: String,
    project_path: PathBuf,
    defines: Vec<(OsString, OsString)>,
    build_type: Option<String>,
}

impl Config {
    pub fn new<P: AsRef<Path>>(project_name: &str, path: P) -> Config {
        Config {
            project_name: project_name.to_string(),
            project_path: path.as_ref().to_owned(),
            defines: Vec::new(),
            build_type: None,
        }
    }

    pub fn define<K: AsRef<OsStr>, V: AsRef<OsStr>>(&mut self, key: K, value: V) -> &mut Config {
        self.defines
            .push((key.as_ref().to_os_string(), value.as_ref().to_os_string()));
        self
    }

    pub fn build_type(&mut self, build_type: &str) -> &mut Config {
        self.build_type = Some(build_type.to_string());
        self
    }

    // /// Configure an environment variable for the `cmake` processes spawned by
    // /// this crate in the `build` step.
    // pub fn env<K, V>(&mut self, key: K, value: V) -> &mut Config
    // where
    //     K: AsRef<OsStr>,
    //     V: AsRef<OsStr>,
    // {
    //     self.cmake_config.env(key, value);
    //     self
    // }

    pub fn build(&mut self) -> PathBuf {
        let out_dir = match std::env::var("OUT_DIR") {
            Ok(out_dir) => out_dir,
            // if we don't have OUT_DIR, i.e. we're not running in a build.rs
            // (but probably from a test environment), make a build dir in
            // target
            Err(_) => std::env::current_dir()
                .unwrap()
                .join("target")
                .join(&self.project_name)
                .to_string_lossy()
                .to_string(),
        };

        let dst = PathBuf::from(out_dir);
        let build = dst.join("build");
        let _ = fs::create_dir_all(&build);

        self.maybe_clear(&build);

        let build_type = if let Some(build_type) = &self.build_type {
            build_type.clone()
        } else {
            "Release".to_string()
        };

        let mut cmd = std::process::Command::new("cmake");
        cmd.args(["-G", "Ninja"]);
        cmd.arg(format!("-DCMAKE_BUILD_TYPE={}", build_type));
        cmd.arg(format!("-S {}", self.project_path.display()));
        cmd.arg(format!("-B {}", build.display()));
        cmd.arg(format!("-DCMAKE_INSTALL_PREFIX={}", dst.display()));

        for (key, value) in &self.defines {
            cmd.arg(format!(
                "-D{}={}",
                key.to_str().unwrap(),
                value.to_str().unwrap()
            ));
        }

        // configure
        run(&mut cmd);

        // build
        let mut cmd = std::process::Command::new("cmake");
        cmd.args(["--build", &format!("{}", build.display())]);
        cmd.args(["--target", "install"]);

        run(&mut cmd);

        print_link_args(&dst, &self.project_name);

        dst
    }

    // If a cmake project has previously been built (e.g. CMakeCache.txt already
    // exists), then cmake will choke if the source directory for the original
    // project being built has changed. Detect this situation through the
    // `CMAKE_HOME_DIRECTORY` variable that cmake emits and if it doesn't match
    // we blow away the build directory and start from scratch (the recommended
    // solution apparently [1]).
    //
    // [1]: https://cmake.org/pipermail/cmake/2012-August/051545.html
    //
    // This is take from: https://github.com/rust-lang/cmake-rs
    // Licensed under Apache-2.0
    fn maybe_clear(&self, dir: &Path) {
        use std::io::Read;
        // CMake will apparently store canonicalized paths which normally
        // isn't relevant to us but we canonicalize it here to ensure
        // we're both checking the same thing.
        let path = fs::canonicalize(&self.project_path).unwrap_or_else(|_| self.project_path.clone());
        let mut f = match std::fs::File::open(dir.join("CMakeCache.txt")) {
            Ok(f) => f,
            Err(..) => return,
        };
        let mut u8contents = Vec::new();
        match f.read_to_end(&mut u8contents) {
            Ok(f) => f,
            Err(..) => return,
        };
        let contents = String::from_utf8_lossy(&u8contents);
        drop(f);
        for line in contents.lines() {
            if line.starts_with("CMAKE_HOME_DIRECTORY") {
                let needs_cleanup = match line.split('=').next_back() {
                    Some(cmake_home) => fs::canonicalize(cmake_home)
                        .ok()
                        .map(|cmake_home| cmake_home != path)
                        .unwrap_or(true),
                    None => true,
                };
                if needs_cleanup {
                    println!(
                        "detected home dir change, cleaning out entire build \
                         directory"
                    );
                    fs::remove_dir_all(dir).unwrap();
                }
                break;
            }
        }
    }
}

fn run(cmd: &mut std::process::Command) {
    println!("running {:?}", cmd);
    let status = match cmd.status() {
        Ok(status) => status,
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => {
            panic!(
                "failed to execute command: {}\nis `cmake` not installed or not in the path?",
                e,
            );
        }
        Err(e) => panic!("failed to execute command: {}", e),
    };
    if !status.success() {
        panic!("command did not execute successfully, got: {}", status);
    }
}

#[cfg(test)]
mod tests {
    use crate::*;

    #[test]
    pub fn test01() {
        let dst = Config::new("openusd", "../bbl-usd")
            .define("BBL_LANGUAGES", "rust")
            .build();

        println!("{}", dst.display());
    }
}
