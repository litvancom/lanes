//! Board access levels. The capability ladder is view < comment < edit < own.
//! `viewer`, `commenter`, `editor` are invitable; `owner` is the board creator only.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    Viewer,
    Commenter,
    Editor,
    Owner,
}

impl Role {
    /// Parse the stored DB string into a Role. Returns None for unknown strings.
    pub fn parse(s: &str) -> Option<Role> {
        match s {
            "viewer" => Some(Role::Viewer),
            "commenter" => Some(Role::Commenter),
            "editor" => Some(Role::Editor),
            "owner" => Some(Role::Owner),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Viewer => "viewer",
            Role::Commenter => "commenter",
            Role::Editor => "editor",
            Role::Owner => "owner",
        }
    }

    /// True for the three roles a board owner may grant via invite/manage.
    pub fn is_invitable(&self) -> bool {
        matches!(self, Role::Viewer | Role::Commenter | Role::Editor)
    }

    pub fn can_comment(&self) -> bool {
        matches!(self, Role::Commenter | Role::Editor | Role::Owner)
    }

    pub fn can_edit(&self) -> bool {
        matches!(self, Role::Editor | Role::Owner)
    }

    pub fn is_owner(&self) -> bool {
        matches!(self, Role::Owner)
    }
}

#[cfg(test)]
mod tests {
    use super::Role;

    #[test]
    fn parse_round_trip() {
        for s in ["viewer", "commenter", "editor", "owner"] {
            assert_eq!(Role::parse(s).unwrap().as_str(), s);
        }
        assert_eq!(Role::parse("member"), None);
        assert_eq!(Role::parse(""), None);
    }

    #[test]
    fn capability_ladder() {
        assert!(!Role::Viewer.can_comment() && !Role::Viewer.can_edit() && !Role::Viewer.is_owner());
        assert!(Role::Commenter.can_comment() && !Role::Commenter.can_edit());
        assert!(Role::Editor.can_comment() && Role::Editor.can_edit() && !Role::Editor.is_owner());
        assert!(Role::Owner.can_comment() && Role::Owner.can_edit() && Role::Owner.is_owner());
    }

    #[test]
    fn invitable_excludes_owner() {
        assert!(Role::Viewer.is_invitable() && Role::Commenter.is_invitable() && Role::Editor.is_invitable());
        assert!(!Role::Owner.is_invitable());
    }
}
