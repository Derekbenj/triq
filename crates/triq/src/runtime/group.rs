//! Groups: nodes that belong together.
//!
//! A node has room for the name of one group. The runtime keeps the name
//! and reports it, and does nothing else with it: a tool that draws the
//! machine draws the nodes of a group as one. A group inside another is
//! written with the outer one first, `outer/inner`. A group is there for as
//! long as a node says it is in it.
//!
//! A runtime can also offer a group as a kind: several nodes of kinds it
//! has registered, wired to each other, and created together by one request.
//! The rest of the runtime does not depend on this file.
use super::{Factory, Inner, Runtime, check_name};
use crate::{Direction, Error, Group, PortSpec, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

impl Runtime {
    /// Put a node in a group, or with an empty name in none. A group inside
    /// another is `outer/inner`. Nothing about how the node runs changes.
    pub fn set_group(&self, node: &str, group: &str) -> Result {
        self.inner.set_group(node, group)
    }

    /// Offer a group of nodes under the name `kind`: any program may then
    /// ask this runtime for one, as it asks for a node. Its nodes are of
    /// kinds that were registered before it.
    ///
    /// A group that is asked for under the name `drone` is the nodes
    /// `drone.estimator`, `drone.controller`, and so on, each in the group
    /// `drone`, with the settings and on the threads that `group` gives
    /// them, and wired as it says. The placement and the settings in the
    /// request itself are not used.
    pub fn register_group(&self, kind: &str, group: Group) -> Result {
        self.inner.check_group(&group)?;
        let factory = Factory {
            ports: Vec::new(),
            params: Value::Null,
            info: Mutex::default(),
            local: false,
            group: Some(group.clone()),
            spawn: Box::new(move |inner, name, _placement, _params| inner.spawn_group(name, &group)),
        };
        self.inner.offer(kind, factory)
    }
}

impl Inner {
    pub(crate) fn set_group(&self, node: &str, group: &str) -> Result {
        if !group.is_empty() {
            group.split('/').try_for_each(|part| check_name("group", part))?;
        }
        let mut state = self.state.lock().unwrap();
        let unknown = || Error::Unknown(format!("node {node:?} in runtime {:?}", self.name));
        state.nodes.get_mut(node).ok_or_else(unknown)?.group = group.into();
        drop(state);
        self.changed();
        Ok(())
    }

    /// What is wrong with a group, if anything is. It is found when the
    /// group is offered, not when one is first asked for.
    fn check_group(&self, group: &Group) -> Result {
        if group.nodes.is_empty() {
            return Err(Error::Mismatch("a group is made of at least one node".into()));
        }
        let state = self.state.lock().unwrap();
        // The ports each node will have, by what the node is called in the group.
        let mut ports: BTreeMap<&str, &[PortSpec]> = BTreeMap::new();
        for member in &group.nodes {
            check_name("node", &member.name)?;
            let unknown = || Error::Unknown(format!("factory {:?} in runtime {:?}", member.kind, self.name));
            let factory = state.factories.get(&member.kind).ok_or_else(unknown)?;
            if factory.group.is_some() {
                return Err(Error::Mismatch(format!("{:?} is a group, and a group is made of nodes", member.kind)));
            }
            if ports.insert(&member.name, &factory.ports).is_some() {
                return Err(Error::Name(format!("two nodes of the group are called {:?}", member.name)));
            }
        }
        // A port of one of those nodes, by `node/port`.
        let port = |name: &str, direction: Direction, what: &str| {
            let found = name.split_once('/').and_then(|(node, port)| {
                ports.get(node)?.iter().find(|spec| spec.name == port && spec.direction == direction)
            });
            found.ok_or_else(|| Error::Unknown(format!("{what} {name:?} in the group")))
        };
        for (from, to) in &group.wires {
            let input = port(to, Direction::Input, "input")?;
            // An output that is not of the group is looked at when the group is made.
            if from.split('/').count() == 3 {
                continue;
            }
            let output = port(from, Direction::Output, "output")?;
            if output.message != input.message {
                return Err(Error::Mismatch(format!(
                    "{from} sends {} and {to} takes {}",
                    output.message, input.message
                )));
            }
        }
        Ok(())
    }

    /// Create the nodes of a group under the name `name`, and wire them.
    fn spawn_group(self: &Arc<Self>, name: &str, group: &Group) -> Result {
        check_name("group", name)?;
        let mut made = Vec::new();
        let nodes = group.nodes.iter().try_for_each(|member| {
            let node = member.named(name);
            self.spawn_kind(&member.kind, &node, member.placement.clone(), member.params.clone())?;
            made.push(node.clone());
            self.set_group(&node, name)
        });
        let whole = nodes.and_then(|()| group.wired(name).iter().try_for_each(|(from, to)| self.connect(from, to)));
        // A group is made whole or not at all.
        if whole.is_err() {
            made.iter().for_each(|node| _ = self.remove(node));
        }
        whole
    }
}
