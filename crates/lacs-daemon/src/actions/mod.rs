use lacs_types::RiskLevel;

pub mod containers;
pub mod deployment;
pub mod flatpak;
pub mod identity;
pub mod layering;
pub mod network;
pub mod package_repos;
pub mod services;
pub mod toolbox;
pub mod users;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionMechanism {
    Command {
        program: &'static str,
        args: Vec<String>,
    },
    FileScan {
        path: String,
    },
    FileWrite {
        path: String,
        content: String,
    },
    FilePatch {
        path: String,
        search: String,
        replace: String,
    },
    FileDelete {
        path: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionSpec {
    pub action_name: &'static str,
    pub mechanism: ActionMechanism,
    pub risk_level: RiskLevel,
    pub reboot_required: bool,
    pub rollback_available: bool,
}

fn command(
    program: &'static str,
    args: impl IntoIterator<Item = impl Into<String>>,
) -> ActionMechanism {
    ActionMechanism::Command {
        program,
        args: args.into_iter().map(Into::into).collect(),
    }
}

fn file_write(path: impl Into<String>, content: impl Into<String>) -> ActionMechanism {
    ActionMechanism::FileWrite {
        path: path.into(),
        content: content.into(),
    }
}

fn file_patch(
    path: impl Into<String>,
    search: impl Into<String>,
    replace: impl Into<String>,
) -> ActionMechanism {
    ActionMechanism::FilePatch {
        path: path.into(),
        search: search.into(),
        replace: replace.into(),
    }
}

fn file_delete(path: impl Into<String>) -> ActionMechanism {
    ActionMechanism::FileDelete { path: path.into() }
}

pub(crate) fn command_mechanism(
    program: &'static str,
    args: impl IntoIterator<Item = impl Into<String>>,
) -> ActionMechanism {
    command(program, args)
}

pub(crate) fn file_write_mechanism(
    path: impl Into<String>,
    content: impl Into<String>,
) -> ActionMechanism {
    file_write(path, content)
}

pub(crate) fn file_patch_mechanism(
    path: impl Into<String>,
    search: impl Into<String>,
    replace: impl Into<String>,
) -> ActionMechanism {
    file_patch(path, search, replace)
}

pub(crate) fn file_scan_mechanism(path: impl Into<String>) -> ActionMechanism {
    ActionMechanism::FileScan { path: path.into() }
}

pub(crate) fn file_delete_mechanism(path: impl Into<String>) -> ActionMechanism {
    file_delete(path)
}
