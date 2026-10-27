//! Windows OpenSSL adapter: valid openssldir and self-contained static debug symbols.
//! Remove when upstream supplies both without a shared MSVC PDB.
use std::{env, ffi::OsString, fs, path::PathBuf, process::Command};

fn main() {
    let real =
        env::var_os("TIDETERM_BUILD_PERL").expect("prepare-windows-build.ps1 must run first");
    let mut args: Vec<OsString> = env::args_os().skip(1).collect();
    let mut static_symbols = false;
    if args.first().is_some_and(|a| {
        PathBuf::from(a)
            .file_name()
            .is_some_and(|n| n == "Configure")
    }) {
        // Upstream install_dev assumes /Zi always produced a static PDB.
        // Keep its installation conditional now that the objects contain it.
        let template_path = "Configurations/windows-makefile.tmpl";
        let template = fs::read_to_string(template_path)
            .expect("read local OpenSSL Windows makefile template")
            .replace("\r\n", "\n");
        let pdb_copy = concat!(
            "@if \"$(SHLIBS)\"==\"\" \\\n",
            "\t \"$(PERL)\" \"$(SRCDIR)\\util\\copy.pl\" ossl_static.pdb"
        );
        let optional_pdb = pdb_copy.replace("==\"\" \\\n", "==\"\" if exist ossl_static.pdb \\\n");
        assert_eq!(
            template.matches(pdb_copy).count() + template.matches(&optional_pdb).count(),
            1,
            "OpenSSL PDB install rule changed"
        );
        if template.contains(pdb_copy) {
            fs::write(template_path, template.replacen(pdb_copy, &optional_pdb, 1))
                .expect("update local OpenSSL PDB install rule");
        }
        let config_dir = PathBuf::from(env::var_os("ProgramData").expect("Windows ProgramData"))
            .join("TideTerm/OpenSSL");
        for arg in &mut args {
            let target = arg.to_string_lossy();
            if target.starts_with("VC-") {
                assert!(target
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-'));
                let name = format!("{target}-TideTerm");
                // Extend OpenSSL's target instead of overriding its optimization,
                // CRT, security or warning flags. /Z7 replaces only /Zi + /Fd.
                fs::write(
                    "Configurations/99-tideterm-debug.conf",
                    format!(
                        r#"my %targets = (
    "{name}" => {{
        inherit_from => ["{target}"],
        lib_cflags => sub {{
            my $flags = join " ", @_;
            $flags =~ s!/Zi\b!/Z7!g;
            $flags =~ s!/Fdossl_static[.]pdb\b!!g;
            return $flags;
        }},
    }},
);
"#
                    ),
                )
                .expect("write local OpenSSL static-symbol configuration");
                *arg = name.into();
                static_symbols = true;
            }
            if arg.to_string_lossy().starts_with("--openssldir=") {
                *arg = format!(
                    "--openssldir={}",
                    config_dir.to_string_lossy().replace('\\', "/")
                )
                .into();
            }
        }
    }
    let status = Command::new(real)
        .args(args)
        .status()
        .expect("launch build Perl");
    if static_symbols && status.success() {
        if let Some(output) = env::var_os("OUT_DIR") {
            let executable = env::current_exe().expect("OpenSSL adapter executable");
            let version = executable.file_name().expect("versioned adapter filename");
            fs::write(
                PathBuf::from(output).join("tideterm-adapter-version"),
                version.to_string_lossy().as_bytes(),
            )
            .expect("record OpenSSL static-symbol configuration");
        }
    }
    std::process::exit(status.code().unwrap_or(1));
}
