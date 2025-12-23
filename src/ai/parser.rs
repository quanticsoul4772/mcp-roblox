//! Luau code parser for extracting relationships between scripts.

use regex::Regex;

/// Parsed relationships from Luau code.
#[derive(Debug, Clone, Default)]
pub struct LuauRelationships {
    /// require() calls to other modules
    pub requires: Vec<RequireRelation>,
    /// Remote event/function calls
    pub remote_calls: Vec<RemoteCallRelation>,
    /// Event connections
    pub event_connections: Vec<EventConnectionRelation>,
    /// Instance modifications
    pub instance_modifications: Vec<InstanceModification>,
}

/// A require() call to another module.
#[derive(Debug, Clone)]
pub struct RequireRelation {
    /// Line number (1-indexed)
    pub line: usize,
    /// Module path (e.g., "game.ReplicatedStorage.Modules.Combat")
    pub module_path: String,
}

/// A remote event/function call.
#[derive(Debug, Clone)]
pub struct RemoteCallRelation {
    /// Line number
    pub line: usize,
    /// Remote path
    pub remote_path: String,
    /// Method called (FireServer, InvokeServer, etc.)
    pub method: String,
}

/// An event connection.
#[derive(Debug, Clone)]
pub struct EventConnectionRelation {
    /// Line number
    pub line: usize,
    /// Event path
    pub event_path: String,
    /// Method called (Connect, Once, etc.)
    pub method: String,
}

/// An instance modification.
#[derive(Debug, Clone)]
pub struct InstanceModification {
    /// Line number
    pub line: usize,
    /// Instance path
    pub instance_path: String,
    /// Operation type (create, destroy, set_property)
    pub operation: String,
}

/// Parser for extracting relationships from Luau code.
pub struct LuauParser {
    require_pattern: Regex,
    remote_call_pattern: Regex,
    event_connect_pattern: Regex,
    instance_new_pattern: Regex,
}

impl LuauParser {
    /// Create a new Luau parser.
    pub fn new() -> Self {
        Self {
            // require(game.ReplicatedStorage.Modules.Combat)
            // require(script.Parent.Utils)
            require_pattern: Regex::new(
                r#"require\s*\(\s*([a-zA-Z_][a-zA-Z0-9_.]*)\s*\)"#
            ).expect("Invalid require pattern"),

            // RemoteEvent:FireServer(), RemoteFunction:InvokeServer()
            remote_call_pattern: Regex::new(
                r#"([a-zA-Z_][a-zA-Z0-9_.]*)\s*:\s*(Fire(?:Server|Client|AllClients)|Invoke(?:Server|Client))\s*\("#
            ).expect("Invalid remote call pattern"),

            // event:Connect(function), event.Changed:Connect()
            event_connect_pattern: Regex::new(
                r#"([a-zA-Z_][a-zA-Z0-9_.]*)\s*:\s*(Connect|Once|Wait)\s*\("#
            ).expect("Invalid event connect pattern"),

            // Instance.new("Part")
            instance_new_pattern: Regex::new(
                r#"Instance\.new\s*\(\s*["']([^"']+)["']\s*\)"#
            ).expect("Invalid instance new pattern"),
        }
    }

    /// Parse Luau code and extract relationships.
    pub fn parse(&self, content: &str) -> LuauRelationships {
        let mut relationships = LuauRelationships::default();

        for (line_idx, line) in content.lines().enumerate() {
            let line_num = line_idx + 1;

            // Skip comments
            let trimmed = line.trim();
            if trimmed.starts_with("--") {
                continue;
            }

            // Extract requires
            for cap in self.require_pattern.captures_iter(line) {
                if let Some(module_path) = cap.get(1) {
                    relationships.requires.push(RequireRelation {
                        line: line_num,
                        module_path: module_path.as_str().to_string(),
                    });
                }
            }

            // Extract remote calls
            for cap in self.remote_call_pattern.captures_iter(line) {
                if let (Some(path), Some(method)) = (cap.get(1), cap.get(2)) {
                    relationships.remote_calls.push(RemoteCallRelation {
                        line: line_num,
                        remote_path: path.as_str().to_string(),
                        method: method.as_str().to_string(),
                    });
                }
            }

            // Extract event connections
            for cap in self.event_connect_pattern.captures_iter(line) {
                if let (Some(path), Some(method)) = (cap.get(1), cap.get(2)) {
                    // Skip common false positives
                    let path_str = path.as_str();
                    if !path_str.ends_with("Service") && !path_str.contains("game.") {
                        relationships.event_connections.push(EventConnectionRelation {
                            line: line_num,
                            event_path: path_str.to_string(),
                            method: method.as_str().to_string(),
                        });
                    }
                }
            }

            // Extract instance creations
            for cap in self.instance_new_pattern.captures_iter(line) {
                if let Some(class_name) = cap.get(1) {
                    relationships.instance_modifications.push(InstanceModification {
                        line: line_num,
                        instance_path: class_name.as_str().to_string(),
                        operation: "create".to_string(),
                    });
                }
            }
        }

        relationships
    }
}

impl Default for LuauParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_require() {
        let parser = LuauParser::new();
        let content = r#"
            local Combat = require(game.ReplicatedStorage.Modules.Combat)
            local Utils = require(script.Parent.Utils)
        "#;

        let rel = parser.parse(content);
        assert_eq!(rel.requires.len(), 2);
        assert_eq!(rel.requires[0].module_path, "game.ReplicatedStorage.Modules.Combat");
        assert_eq!(rel.requires[1].module_path, "script.Parent.Utils");
    }

    #[test]
    fn test_parse_remote_calls() {
        let parser = LuauParser::new();
        let content = r#"
            DamageRemote:FireServer(target, damage)
            ShopRemote:InvokeServer("buy", itemId)
            BroadcastRemote:FireAllClients(message)
        "#;

        let rel = parser.parse(content);
        assert_eq!(rel.remote_calls.len(), 3);
        assert_eq!(rel.remote_calls[0].method, "FireServer");
        assert_eq!(rel.remote_calls[1].method, "InvokeServer");
        assert_eq!(rel.remote_calls[2].method, "FireAllClients");
    }

    #[test]
    fn test_parse_event_connections() {
        let parser = LuauParser::new();
        let content = r#"
            button.Activated:Connect(function()
                print("clicked")
            end)
            humanoid.Died:Once(onDeath)
        "#;

        let rel = parser.parse(content);
        assert_eq!(rel.event_connections.len(), 2);
        assert_eq!(rel.event_connections[0].event_path, "button.Activated");
        assert_eq!(rel.event_connections[0].method, "Connect");
    }

    #[test]
    fn test_parse_instance_new() {
        let parser = LuauParser::new();
        let content = r#"
            local part = Instance.new("Part")
            local folder = Instance.new('Folder')
        "#;

        let rel = parser.parse(content);
        assert_eq!(rel.instance_modifications.len(), 2);
        assert_eq!(rel.instance_modifications[0].instance_path, "Part");
        assert_eq!(rel.instance_modifications[1].instance_path, "Folder");
    }

    #[test]
    fn test_skip_comments() {
        let parser = LuauParser::new();
        let content = r#"
            -- local Combat = require(game.ReplicatedStorage.Modules.Combat)
            local Utils = require(script.Parent.Utils)
        "#;

        let rel = parser.parse(content);
        assert_eq!(rel.requires.len(), 1);
        assert_eq!(rel.requires[0].module_path, "script.Parent.Utils");
    }

    #[test]
    fn test_complex_script() {
        let parser = LuauParser::new();
        let content = r#"
-- Combat Module
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local Players = game:GetService("Players")

local Damage = require(game.ReplicatedStorage.Modules.Damage)
local Effects = require(script.Parent.Effects)

local CombatRemote = ReplicatedStorage.Remotes.Combat

local function onAttack(player, target)
    local damage = Damage.calculate(player, target)
    CombatRemote:FireClient(target.Player, "hit", damage)

    local hitEffect = Instance.new("Part")
    hitEffect.Parent = workspace
end

CombatRemote.OnServerEvent:Connect(onAttack)
        "#;

        let rel = parser.parse(content);

        assert_eq!(rel.requires.len(), 2);
        assert_eq!(rel.remote_calls.len(), 1);
        assert_eq!(rel.remote_calls[0].method, "FireClient");
        assert_eq!(rel.instance_modifications.len(), 1);
        assert_eq!(rel.instance_modifications[0].instance_path, "Part");
        assert_eq!(rel.event_connections.len(), 1);
    }
}
