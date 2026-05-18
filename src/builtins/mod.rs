mod alias;
mod basic;
mod bind;
pub mod cd;
mod complete;
mod control;
mod declare;
mod dirs;
mod echo;
mod enable_shopt;
mod exec_cmds;
mod fc;
mod getopts;
mod hash;
pub mod helpers;
mod info;
mod kill;
mod mapfile;
mod printf;
mod pwd;
mod read;
pub mod registry;
mod simple_cmds;
mod test_cmd;
mod ulimit;
mod vars;

pub use crate::shell::DpShell;
pub type ShellState = DpShell;

pub use alias::{builtin_alias, builtin_unalias};
pub use basic::{builtin_colon, builtin_history};
pub use bind::builtin_bind;
pub use complete::{builtin_compgen, builtin_complete, builtin_compopt};
pub use control::{builtin_break, builtin_continue, builtin_shift, builtin_trap, builtin_wait};
pub use declare::{builtin_declare, builtin_let};
pub use dirs::{builtin_dirs, builtin_popd, builtin_pushd, builtin_umask};
pub use echo::builtin_echo;
pub use enable_shopt::{builtin_enable, builtin_shopt};
pub use exec_cmds::{builtin_eval, builtin_exec, read_source_file};
pub use fc::builtin_fc;
pub use getopts::builtin_getopts;
pub use hash::builtin_hash;
pub use info::{builtin_command_v, builtin_help, builtin_type};
pub use kill::builtin_kill;
pub use mapfile::{builtin_mapfile, builtin_readarray};
pub use printf::builtin_printf;
pub use pwd::builtin_pwd;
pub use read::builtin_read;
#[allow(unused_imports)]
pub use simple_cmds::{
    builtin_caller, builtin_exit, builtin_false, builtin_return, builtin_suspend, builtin_times,
    builtin_true, builtin_unimp,
};
pub use test_cmd::builtin_test;
pub use ulimit::builtin_ulimit;
pub use vars::{builtin_export, builtin_local, builtin_readonly, builtin_set, builtin_unset};
