use super::{ensure_init, env, with_config, MobileTarget};
use crate::Result;
use tauri_mobile::os;

pub fn command() -> Result<()> {
  with_config(
    Some(Default::default()),
    |_root_conf, config, _metadata, _cli_options| {
      ensure_init(config.project_dir(), MobileTarget::Android)?;
      let env = env()?;
      os::open_file_with("Android Studio", config.project_dir(), &env.base).map_err(Into::into)
    },
  )
}
