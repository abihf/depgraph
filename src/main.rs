use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::{collections::LinkedList, io::{SeekFrom, Write}, sync::Arc, vec};
use swc::{
    common::{comments::NoopComments, source_map::SourceMap, FileName, FilePathMapping},
    config::IsModule,
    ecmascript::ast::EsVersion,
    try_with_handler, Compiler,
};
use swc_ecma_dep_graph::{analyze_dependencies, DependencyDescriptor, DependencyKind};
use swc_ecma_parser::{EsConfig, Syntax, TsConfig};
use tokio::{
    fs::File,
    io::{AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, BufReader},
    sync::{OwnedSemaphorePermit, Semaphore, Mutex},
};

#[tokio::main]
async fn main() -> Result<()> {
    let parallel = std::env::var("DEPGRAPH_PARALLEL").unwrap_or("1000".to_string());
    let semaphore = Arc::new(Semaphore::new(parallel.parse()?));

    let mut stdin = BufReader::new(tokio::io::stdin());
    let stdout = Arc::new(Mutex::new(std::io::stdout()));

    let cm = Arc::new(SourceMap::new(FilePathMapping::empty()));
    let compiler = Arc::new(Compiler::new(cm));

    let mut handlers = LinkedList::new();
    loop {
        let mut file_name = String::new();
        let size = stdin.read_line(&mut file_name).await?;
        if size == 0 {
            break;
        }

        let c = compiler.clone();
        let permit = semaphore.clone().acquire_owned().await?;
        let stdout = stdout.clone();

        handlers.push_back(tokio::spawn(async move {
            let file_name = file_name.trim();

            let val = match analyze(&c, file_name, permit).await {
                Ok(deps) => Value::Array(
                    deps.iter()
                        .map(|dep| {
                            let loc = c.cm.lookup_char_pos(dep.span.lo);
                            let name = dep.specifier.to_string();
                            let mut kind: i32 = match dep.kind {
                                DependencyKind::Require => 0,
                                DependencyKind::Import => 1,
                                DependencyKind::Export => 2,
                                DependencyKind::ImportType => 5,
                                DependencyKind::ExportType => 6,
                            };
                            if dep.is_dynamic {
                                kind = kind | 8;
                            }
                            json!({
                                "k": kind,
                                "n": name,
                                "l": loc.line,
                                "c": loc.col.0
                            })
                        })
                        .collect(),
                ),
                Err(err) => json!(format!("{}", err)),
            };

            let json_line = json!([file_name, val]);
            let mut stdout = stdout.lock().await;
            serde_json::to_writer(stdout.by_ref(), &json_line)?;
            stdout.write(b"\n")?;

            anyhow::Ok(())
        }));
    }

    for handle in handlers {
        if let Err(e) = handle.await {
            eprintln!("{}", e)
        }
    }

    Ok(())
}

async fn analyze(
    c: &Compiler,
    file_name: &str,
    permit: OwnedSemaphorePermit,
) -> Result<Vec<DependencyDescriptor>> {
    let mut file = File::open(file_name)
        .await
        .context(format!("can not open file {}", file_name))?;

    let size = file.seek(SeekFrom::End(0)).await?;
    file.seek(SeekFrom::Start(0)).await?;

    let mut buf = String::with_capacity(size.try_into()?);
    file.read_to_string(&mut buf)
        .await
        .context(format!("can not read file {}", file_name))?;

    drop(file);
    drop(permit);

    try_with_handler(c.cm.clone(), false, |handler| {
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

        let fm = c.cm.new_source_file(FileName::Real(file_name.into()), buf);
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
