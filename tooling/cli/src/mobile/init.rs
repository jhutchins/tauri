// Copyright 2019-2021 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use super::{get_app, Target};
use crate::helpers::{config::get as get_tauri_config, template::JsonMap};
use crate::Result;
use handlebars::{Context, Handlebars, Helper, HelperResult, Output, RenderContext, RenderError};
use tauri_mobile::{
  android::{
    config::Config as AndroidConfig, env::Env as AndroidEnv, target::Target as AndroidTarget,
  },
  config::app::App,
  dot_cargo,
  target::TargetTrait as _,
  util::{
    self,
    cli::{Report, TextWrapper},
  },
};

use std::{env::current_dir, path::PathBuf};

pub fn command(target: Target, ci: bool, reinstall_deps: bool) -> Result<()> {
  let wrapper = TextWrapper::with_splitter(textwrap::termwidth(), textwrap::NoHyphenation);
  exec(
    target,
    &wrapper,
    ci || std::env::var("CI").is_ok(),
    reinstall_deps,
  )
  .map_err(|e| anyhow::anyhow!("{:#}", e))?;
  Ok(())
}

pub fn init_dot_cargo(app: &App, android: Option<(&AndroidEnv, &AndroidConfig)>) -> Result<()> {
  let mut dot_cargo = dot_cargo::DotCargo::load(app)?;
  // Mysteriously, builds that don't specify `--target` seem to fight over
  // the build cache with builds that use `--target`! This means that
  // alternating between i.e. `cargo run` and `cargo apple run` would
  // result in clean builds being made each time you switched... which is
  // pretty nightmarish. Specifying `build.target` in `.cargo/config`
  // fortunately has the same effect as specifying `--target`, so now we can
  // `cargo run` with peace of mind!
  //
  // This behavior could be explained here:
  // https://doc.rust-lang.org/cargo/reference/config.html#buildrustflags
  dot_cargo.set_default_target(util::host_target_triple()?);

  if let Some((env, config)) = android {
    for target in AndroidTarget::all().values() {
      dot_cargo.insert_target(
        target.triple.to_owned(),
        target.generate_cargo_config(config, env)?,
      );
    }
  }

  dot_cargo.write(app).map_err(Into::into)
}

pub fn exec(
  target: Target,
  wrapper: &TextWrapper,
  #[allow(unused_variables)] non_interactive: bool,
  #[allow(unused_variables)] reinstall_deps: bool,
) -> Result<App> {
  let tauri_config = get_tauri_config(None)?;
  let tauri_config_guard = tauri_config.lock().unwrap();
  let tauri_config_ = tauri_config_guard.as_ref().unwrap();

  let app = get_app(tauri_config_);

  let (handlebars, mut map) = handlebars(&app);

  let mut args = std::env::args_os();
  let tauri_binary = args
    .next()
    .map(|bin| {
      let path = PathBuf::from(&bin);
      if path.exists() {
        if let Ok(dir) = current_dir() {
          let absolute_path = util::prefix_path(dir, path);
          return absolute_path.into();
        }
      }
      bin
    })
    .unwrap_or_else(|| std::ffi::OsString::from("cargo"));
  let mut build_args = Vec::new();
  for arg in args {
    let path = PathBuf::from(&arg);
    if path.exists() {
      if let Ok(dir) = current_dir() {
        let absolute_path = util::prefix_path(dir, path);
        build_args.push(absolute_path.to_string_lossy().into_owned());
        continue;
      }
    }
    build_args.push(arg.to_string_lossy().into_owned());
    if arg == "android" || arg == "ios" {
      break;
    }
  }
  build_args.push(target.ide_build_script_name().into());
  map.insert("tauri-binary", tauri_binary.to_string_lossy());
  map.insert("tauri-binary-args", &build_args);
  map.insert("tauri-binary-args-str", build_args.join(" "));

  let app = match target {
    // Generate Android Studio project
    Target::Android => match AndroidEnv::new() {
      Ok(env) => {
        let (app, config, metadata) =
          super::android::get_config(Some(app), tauri_config_, &Default::default());
        map.insert("android", &config);
        super::android::project::gen(&config, &metadata, (handlebars, map), wrapper)?;
        init_dot_cargo(&app, Some((&env, &config)))?;
        app
      }
      Err(err) => {
        if err.sdk_or_ndk_issue() {
          Report::action_request(
            " to initialize Android environment; Android support won't be usable until you fix the issue below and re-run `tauri android init`!",
            err,
          )
          .print(wrapper);
          init_dot_cargo(&app, None)?;
          app
        } else {
          return Err(err.into());
        }
      }
    },
    #[cfg(target_os = "macos")]
    // Generate Xcode project
    Target::Ios => {
      let (app, config, metadata) =
        super::ios::get_config(Some(app), tauri_config_, &Default::default());
      map.insert("apple", &config);
      super::ios::project::gen(
        &config,
        &metadata,
        (handlebars, map),
        wrapper,
        non_interactive,
        reinstall_deps,
      )?;
      init_dot_cargo(&app, None)?;
      app
    }
  };

  Report::victory(
    "Project generated successfully!",
    "Make cool apps! 🌻 🐕 🎉",
  )
  .print(wrapper);
  Ok(app)
}

fn handlebars(app: &App) -> (Handlebars<'static>, JsonMap) {
  let mut h = Handlebars::new();
  h.register_escape_fn(handlebars::no_escape);

  h.register_helper("html-escape", Box::new(html_escape));
  h.register_helper("join", Box::new(join));
  h.register_helper("quote-and-join", Box::new(quote_and_join));
  h.register_helper(
    "quote-and-join-colon-prefix",
    Box::new(quote_and_join_colon_prefix),
  );
  h.register_helper("snake-case", Box::new(snake_case));
  h.register_helper("reverse-domain", Box::new(reverse_domain));
  h.register_helper(
    "reverse-domain-snake-case",
    Box::new(reverse_domain_snake_case),
  );
  // don't mix these up or very bad things will happen to all of us
  h.register_helper("prefix-path", Box::new(prefix_path));
  h.register_helper("unprefix-path", Box::new(unprefix_path));

  let mut map = JsonMap::default();
  map.insert("app", app);

  (h, map)
}

fn get_str<'a>(helper: &'a Helper) -> &'a str {
  helper
    .param(0)
    .and_then(|v| v.value().as_str())
    .unwrap_or("")
}

fn get_str_array(helper: &Helper, formatter: impl Fn(&str) -> String) -> Option<Vec<String>> {
  helper.param(0).and_then(|v| {
    v.value().as_array().and_then(|arr| {
      arr
        .iter()
        .map(|val| {
          val.as_str().map(
            #[allow(clippy::redundant_closure)]
            |s| formatter(s),
          )
        })
        .collect()
    })
  })
}

fn html_escape(
  helper: &Helper,
  _: &Handlebars,
  _ctx: &Context,
  _: &mut RenderContext,
  out: &mut dyn Output,
) -> HelperResult {
  out
    .write(&handlebars::html_escape(get_str(helper)))
    .map_err(Into::into)
}

fn join(
  helper: &Helper,
  _: &Handlebars,
  _: &Context,
  _: &mut RenderContext,
  out: &mut dyn Output,
) -> HelperResult {
  out
    .write(
      &get_str_array(helper, |s| s.to_string())
        .ok_or_else(|| RenderError::new("`join` helper wasn't given an array"))?
        .join(", "),
    )
    .map_err(Into::into)
}

fn quote_and_join(
  helper: &Helper,
  _: &Handlebars,
  _: &Context,
  _: &mut RenderContext,
  out: &mut dyn Output,
) -> HelperResult {
  out
    .write(
      &get_str_array(helper, |s| format!("{s:?}"))
        .ok_or_else(|| RenderError::new("`quote-and-join` helper wasn't given an array"))?
        .join(", "),
    )
    .map_err(Into::into)
}

fn quote_and_join_colon_prefix(
  helper: &Helper,
  _: &Handlebars,
  _: &Context,
  _: &mut RenderContext,
  out: &mut dyn Output,
) -> HelperResult {
  out
    .write(
      &get_str_array(helper, |s| format!("{:?}", format!(":{s}")))
        .ok_or_else(|| {
          RenderError::new("`quote-and-join-colon-prefix` helper wasn't given an array")
        })?
        .join(", "),
    )
    .map_err(Into::into)
}

fn snake_case(
  helper: &Helper,
  _: &Handlebars,
  _: &Context,
  _: &mut RenderContext,
  out: &mut dyn Output,
) -> HelperResult {
  use heck::ToSnekCase as _;
  out
    .write(&get_str(helper).to_snek_case())
    .map_err(Into::into)
}

fn reverse_domain(
  helper: &Helper,
  _: &Handlebars,
  _: &Context,
  _: &mut RenderContext,
  out: &mut dyn Output,
) -> HelperResult {
  out
    .write(&util::reverse_domain(get_str(helper)))
    .map_err(Into::into)
}

fn reverse_domain_snake_case(
  helper: &Helper,
  _: &Handlebars,
  _: &Context,
  _: &mut RenderContext,
  out: &mut dyn Output,
) -> HelperResult {
  use heck::ToSnekCase as _;
  out
    .write(&util::reverse_domain(get_str(helper)).to_snek_case())
    .map_err(Into::into)
}

fn app_root(ctx: &Context) -> Result<&str, RenderError> {
  let app_root = ctx
    .data()
    .get("app")
    .ok_or_else(|| RenderError::new("`app` missing from template data."))?
    .get("root-dir")
    .ok_or_else(|| RenderError::new("`app.root-dir` missing from template data."))?;
  app_root
    .as_str()
    .ok_or_else(|| RenderError::new("`app.root-dir` contained invalid UTF-8."))
}

fn prefix_path(
  helper: &Helper,
  _: &Handlebars,
  ctx: &Context,
  _: &mut RenderContext,
  out: &mut dyn Output,
) -> HelperResult {
  out
    .write(
      util::prefix_path(app_root(ctx)?, get_str(helper))
        .to_str()
        .ok_or_else(|| {
          RenderError::new(
            "Either the `app.root-dir` or the specified path contained invalid UTF-8.",
          )
        })?,
    )
    .map_err(Into::into)
}

fn unprefix_path(
  helper: &Helper,
  _: &Handlebars,
  ctx: &Context,
  _: &mut RenderContext,
  out: &mut dyn Output,
) -> HelperResult {
  out
    .write(
      util::unprefix_path(app_root(ctx)?, get_str(helper))
        .map_err(|_| {
          RenderError::new("Attempted to unprefix a path that wasn't in the app root dir.")
        })?
        .to_str()
        .ok_or_else(|| {
          RenderError::new(
            "Either the `app.root-dir` or the specified path contained invalid UTF-8.",
          )
        })?,
    )
    .map_err(Into::into)
}
