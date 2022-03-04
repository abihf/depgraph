use anyhow::{anyhow, Context, Result};
use argh::FromArgs;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::LinkedList, io::Write, slice::Iter, sync::Arc, vec};
use swc::{
    common::{comments::NoopComments, source_map::SourceMap, FileName, FilePathMapping},
    config::IsModule,
    ecmascript::ast::{EsVersion, ExportSpecifier, ModuleDecl, ModuleExportName, ModuleItem},
    try_with_handler, Compiler,
};
use swc_ecma_dep_graph::{analyze_dependencies, DependencyKind};
use swc_ecma_parser::{EsConfig, Syntax, TsConfig};
use tokio::{
    fs::File,
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, BufReader, Lines},
    sync::{Mutex, Semaphore},
};

/// depgraph
#[derive(FromArgs)]
struct Cmd {
    /// show version
    #[argh(switch, short = 'v')]
    version: bool,

    /// files to be analyzed
    #[argh(positional)]
    files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DependencyItem {
    #[serde(rename = "k")]
    kind: u8,

    #[serde(rename = "n")]
    name: String,

    #[serde(rename = "l")]
    line: usize,

    #[serde(rename = "c")]
    column: usize,

    #[serde(rename = "e", skip_serializing_if = "Vec::is_empty")]
    exports: Vec<String>,
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() -> Result<()> {
    let cmd: Cmd = argh::from_env();
    if cmd.version {
        println!("{}", VERSION);
        return Ok(());
    }

    let mut files = if cmd.files.len() > 0 {
        FileList::Args(cmd.files.iter())
    } else {
        FileList::Stdin(BufReader::new(tokio::io::stdin()).lines())
    };

    let parallel = std::env::var("DEPGRAPH_PARALLEL").unwrap_or("1000".to_string());
    let semaphore = Arc::new(Semaphore::new(parallel.parse()?));

    let stdout = Arc::new(Mutex::new(std::io::stdout()));

    let mut handlers = LinkedList::new();
    while let Some(file_name) = files.next().await? {
        let permit = semaphore.clone().acquire_owned().await?;
        let stdout = stdout.clone();

        handlers.push_back(tokio::spawn(async move {
            let file_name = file_name.trim();

            let line = match analyze_file(file_name, || drop(permit)).await {
                Ok(deps) => json!([file_name, deps]),
                Err(err) => json!([file_name, format!("{}", err)]),
            };

            let mut stdout = stdout.lock().await;
            serde_json::to_writer(stdout.by_ref(), &line)?;
            stdout.write(b"\n")?;

            anyhow::Ok(())
        }));
    }

    for handle in handlers {
        handle.await??;
    }

    Ok(())
}

const EMPTY_EXPORT: Vec<String> = vec![];

async fn analyze_file<F>(file_name: &str, onload: F) -> Result<Vec<DependencyItem>>
where
    F: FnOnce() -> (),
{
    let mut file = File::open(file_name)
        .await
        .context(format!("can not open file {}", file_name))?;
    let size = file.metadata().await?.len();

    let mut source = String::with_capacity(size.try_into()?);
    file.read_to_string(&mut source)
        .await
        .context(format!("can not read file {}", file_name))?;

    drop(file);
    onload();

    analyze(file_name, source)
}

fn analyze(file_name: &str, source: String) -> Result<Vec<DependencyItem>> {
    let cm = Arc::new(SourceMap::new(FilePathMapping::empty()));
    let c = Arc::new(Compiler::new(cm));
    let fm =
        c.cm.new_source_file(FileName::Real(file_name.into()), source);

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

    let program = try_with_handler(c.cm.clone(), true, |handler| {
        c.parse_js(
            fm,
            handler,
            EsVersion::latest(),
            syntax,
            IsModule::Bool(true),
            false,
        )
    })
    .context("failed to process js file")?;

    let module = program
        .as_module()
        .ok_or(anyhow!("program is not module"))?;

    let mut has_stmt = false;
    let mut deps = Vec::with_capacity(module.body.len());
    for item in &module.body {
        if let ModuleItem::ModuleDecl(decl) = item {
            if let ModuleDecl::ExportNamed(node) = decl {
                if let Some(src) = &node.src {
                    let mut exports = Vec::with_capacity(node.specifiers.len());
                    for e in &node.specifiers {
                        exports.push(match &e {
                            ExportSpecifier::Namespace(ns) => {
                                format!("{}:*", export_name_to_string(&ns.name))
                            }
                            ExportSpecifier::Named(e) => {
                                if let Some(exported) = &e.exported {
                                    format!(
                                        "{}:{}",
                                        export_name_to_string(exported),
                                        export_name_to_string(&e.orig)
                                    )
                                } else {
                                    format!("{0}:{0}", export_name_to_string(&e.orig))
                                }
                            }
                            ExportSpecifier::Default(e) => {
                                format!("default:{}", e.exported)
                            }
                        });
                    }

                    deps.push((src, exports, node.type_only));
                    continue;
                }
            } else if let ModuleDecl::ExportAll(node) = decl {
                deps.push((&node.src, vec![String::from("*:*")], false));
                continue;
            }
        };
        has_stmt = true;
        break;
    }

    if !has_stmt {
        let mut res = Vec::with_capacity(deps.len());
        for (src, exports, type_only) in deps {
            let name = src.value.to_string();
            let loc = c.cm.lookup_char_pos(src.span.lo);
            res.push(DependencyItem {
                kind: if type_only { 6 } else { 2 },
                name,
                line: loc.line,
                column: loc.col.0,
                exports,
            })
        }
        return Ok(res);
    }

    let deps = analyze_dependencies(module, &NoopComments::default());
    let mut res = Vec::with_capacity(deps.len());
    for dep in deps {
        let loc = c.cm.lookup_char_pos(dep.specifier_span.lo);
        let name = dep.specifier.to_string();
        let mut kind: u8 = match dep.kind {
            DependencyKind::Require => 0,
            DependencyKind::Import => 1,
            DependencyKind::Export => 2,
            DependencyKind::ImportType => 5,
            DependencyKind::ExportType => 6,
            DependencyKind::ImportEquals => 1,
            DependencyKind::ExportEquals => 2,
        };
        if dep.is_dynamic {
            kind = kind | 8;
        }
        res.push(DependencyItem {
            kind,
            name,
            line: loc.line,
            column: loc.col.0,
            exports: EMPTY_EXPORT,
        })
    }
    Ok(res)
}

fn export_name_to_string<'a>(e: &'a ModuleExportName) -> &'a str {
    match e {
        ModuleExportName::Ident(i) => i.sym.as_ref(),
        ModuleExportName::Str(s) => s.value.as_ref(),
    }
}

enum FileList<'a, R> {
    Stdin(Lines<R>),
    Args(Iter<'a, String>),
}

impl<'a, R> FileList<'a, R>
where
    R: AsyncBufRead + Unpin,
{
    async fn next(&mut self) -> Result<Option<String>> {
        match self {
            FileList::Stdin(stdin) => Ok(stdin.next_line().await?),
            FileList::Args(args) => Ok(args.next().map(|a| a.clone())),
        }
    }
}

#[cfg(test)]
mod tests {
    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;

    #[test]
    fn test_analyze_export_only() -> Result<()> {
        let source = r#"
            export { a as b } from 'c';
            export * from 'd';
            export { default as x } from 'an';
            "#;
        let deps = analyze("test.js", source.to_string())?;
        assert_eq!(deps[0].name, "c");
        assert_eq!(deps[0].exports[0], "b:a");
        Ok(())
    }
}
