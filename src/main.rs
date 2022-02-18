use anyhow::{anyhow, Context, Result};
use once_cell::sync::OnceCell;
use serde_json::{json, Value};
use std::{io::SeekFrom, path::PathBuf, sync::Arc, vec};
use swc::{
    common::FilePathMapping, config::IsModule, ecmascript::ast::EsVersion, try_with_handler,
    Compiler,
};
use swc_common::{comments::NoopComments, source_map::SourceMap, FileName};
use swc_ecma_dep_graph::{analyze_dependencies, DependencyDescriptor, DependencyKind};
use swc_ecma_parser::{EsConfig, Syntax, TsConfig};
use tokio::{
    fs::File,
    io::{self, AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, BufReader},
    sync::Semaphore,
};

#[tokio::main]
async fn main() -> Result<()> {
    // let mut handlers: Vec<JoinHandle<Result<(String, Vec<DependencyDescriptor>)>>> = vec![];
    let mut handlers = vec![];
    let parallel = std::env::var("DEPGRAPH_PARALLEL").unwrap_or("1000".to_string());
    let semaphore = Arc::new(Semaphore::new(parallel.parse()?));

    let cur_dir = std::env::current_dir()?;
    let mut stdin = BufReader::new(io::stdin());
    loop {
        let mut file_name = String::new();
        let size = stdin.read_line(&mut file_name).await?;
        if size == 0 {
            break;
        }
        let file_name = String::from(file_name.trim());

        let cur_dir = cur_dir.clone();
        let permit = semaphore.clone().acquire_owned().await?;
        handlers.push(tokio::spawn(async move {
            let full_path = &cur_dir.join(file_name.clone());
            let mut file = File::open(full_path).await?;

            let size = file.seek(SeekFrom::End(0)).await?;
            file.seek(SeekFrom::Start(0)).await?;

            let mut buf = String::with_capacity(size.try_into()?);
            file.read_to_string(&mut buf)
                .await
                .context(format!("can not read file {}", file_name.clone()))?;
            drop(permit);
            drop(file);

            let deps = analyze(file_name.clone().into(), buf.as_str())?;
            let res: Result<(String, Vec<DependencyDescriptor>)> = Ok((file_name, deps));
            res
        }));
    }

    let mut list = vec![];
    let c = compiler();
    let sc = &c.cm;

    for res in handlers {
        let (name, deps) = match res.await? {
            Ok(val) => val,
            Err(e) => {
                eprintln!("{}", e);
                continue;
            }
        };
        let deps = deps
            .iter()
            .map(|dep| {
                let loc = sc.lookup_char_pos(dep.span.lo);
                let name = dep.specifier.to_string();
                let kind: i32 = match dep.kind {
                    DependencyKind::Require => 0,
                    DependencyKind::Import => 1,
                    DependencyKind::Export => 2,
                    DependencyKind::ImportType => 5,
                    DependencyKind::ExportType => 6,
                };
                let dynamic: i32 = if dep.is_dynamic { 1 } else { 0 };
                json!({
                    "k": kind,
                    "n": name,
                    "d": dynamic,
                    "l": loc.line,
                    "c": loc.col.0
                })
            })
            .collect();

        list.push(json!([name, Value::Array(deps)]));
    }

    let obj = Value::Array(list);
    serde_json::to_writer(std::io::stdout(), &obj)?;
    Ok(())
}

fn analyze(name: PathBuf, source: &str) -> Result<Vec<DependencyDescriptor>> {
    let c = compiler();

    try_with_handler(c.cm.clone(), false, |handler| {
        let file_name = name.to_str().unwrap_or_default();

        let syntax = if file_name.ends_with(".ts") || file_name.ends_with(".tsx") {
            Syntax::Typescript(TsConfig {
                tsx: file_name.ends_with(".tsx"),
                dts: file_name.ends_with(".d.ts"),
                ..Default::default()
            })
        } else {
            Syntax::Es(EsConfig {
                jsx: true,
                ..Default::default()
            })
        };

        let fm = c.cm.new_source_file(FileName::Real(name), source.into());
        let program = c
            .parse_js(
                fm,
                handler,
                EsVersion::Es2020,
                syntax,
                IsModule::Bool(true),
                false,
            )
            .context("failed to process js file")?;
        let module = program
            .as_module()
            .ok_or(anyhow!("program is not module"))?;

        Ok(analyze_dependencies(module, &NoopComments::default()))
    })
}

fn compiler() -> &'static Compiler {
    static C: OnceCell<Compiler> = OnceCell::new();
    C.get_or_init(|| {
        let cm = Arc::new(SourceMap::new(FilePathMapping::empty()));
        Compiler::new(cm)
    })
}