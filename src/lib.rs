#[macro_use]
extern crate napi_derive;

use napi::Result;
use anyhow::anyhow;
use std::{sync::Arc, vec};
use swc::{
  config::IsModule,
  try_with_handler, Compiler, HandlerOpts,
};
use swc_common::{
  comments::NoopComments, source_map::SourceMap, FileName, FilePathMapping
};
use swc_ecmascript::ast::{
  EsVersion, ExportSpecifier, ModuleDecl, ModuleExportName, ModuleItem
};
use swc_ecma_dep_graph::{analyze_dependencies, DependencyKind};
use swc_ecma_parser::{EsConfig, Syntax, TsConfig};

#[napi(object)]
pub struct Dependency {
  pub kind: u32,
  pub name: String,
  pub line: u32,
  pub column: u32,
  pub exports: Option<Vec<String>>,
}


fn map_err<E: std::fmt::Debug>(e: E) -> napi::Error {
  napi::Error::from_reason(format!("{:?}", e))
}

#[napi]
pub async fn analyze(file_name: String, source: String) -> Result<Vec<Dependency>> {
  tokio::spawn(async move {
    let file_name = file_name.as_str();
    let cm = Arc::new(SourceMap::new(FilePathMapping::empty()));
    let c = Compiler::new(cm);
    let fm = c
      .cm
      .new_source_file(FileName::Real(file_name.into()), source);

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

    let program = try_with_handler(c.cm.clone(), HandlerOpts::default(), |handler| {
      c.parse_js(
        fm,
        handler,
        EsVersion::latest(),
        syntax,
        IsModule::Bool(true),
        None,
      )
    })
    .map_err(map_err)?;

    let module = program
      .as_module()
      .ok_or(anyhow!("program is not module"))
      .map_err(map_err)?;

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
        res.push(Dependency {
          kind: if type_only { 6 } else { 2 },
          name,
          line: loc.line.try_into().map_err(map_err)?,
          column: loc.col.0.try_into().map_err(map_err)?,
          exports: Some(exports),
        })
      }
      return Ok(res);
    }

    let deps = analyze_dependencies(module, &NoopComments::default());
    let mut res = Vec::with_capacity(deps.len());
    for dep in deps {
      let loc = c.cm.lookup_char_pos(dep.specifier_span.lo);
      let name = dep.specifier.to_string();
      let mut kind: u32 = match dep.kind {
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
      res.push(Dependency {
        kind,
        name,
        line: loc.line.try_into().map_err(map_err)?,
        column: loc.col.0.try_into().map_err(map_err)?,
        exports: None,
      })
    }
    Ok(res)
  })
  .await
  .map_err(map_err)?
}

fn export_name_to_string<'a>(e: &'a ModuleExportName) -> &'a str {
  match e {
    ModuleExportName::Ident(i) => i.sym.as_ref(),
    ModuleExportName::Str(s) => s.value.as_ref(),
  }
}
