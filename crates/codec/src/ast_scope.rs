//! Parsed JavaScript binding facts used by the constrained codec recognizers.
//! No source is executed. Oxc owns grammar, lexical scopes and write targets.
use oxc_allocator::Allocator;
use oxc_ast::{AstKind, ast::*};
use oxc_parser::Parser;
use oxc_semantic::{Semantic, SemanticBuilder};
use oxc_span::{GetSpan, SourceType, Span};
use oxc_syntax::{node::NodeId, scope::ScopeId, symbol::SymbolId};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug)]
enum Value {
    Number(i64),
    Boolean(bool),
    EmptyArray,
    Object,
}
impl Value {
    fn number(self) -> Option<i64> {
        match self {
            Self::Number(n) => Some(n),
            Self::Boolean(b) => Some(i64::from(b)),
            Self::EmptyArray => Some(0),
            Self::Object => None,
        }
    }
    fn truth(self) -> bool {
        match self {
            Self::Number(n) => n != 0,
            Self::Boolean(b) => b,
            Self::EmptyArray | Self::Object => true,
        }
    }
}
#[derive(Clone, Default)]
pub(super) struct Numbers {
    values: HashMap<usize, i64>,
    arrays: HashMap<usize, Vec<Option<i64>>>,
    arguments: HashMap<usize, i64>,
    legacy: Option<BTreeMap<String, i64>>,
}
impl Numbers {
    pub fn legacy(values: BTreeMap<String, i64>) -> Self {
        Self {
            legacy: Some(values),
            ..Self::default()
        }
    }
    pub fn is_legacy(&self) -> bool {
        self.legacy.is_some()
    }
    pub fn get(&self, token: &str) -> Option<&i64> {
        self.legacy
            .as_ref()
            .and_then(|m| m.get(token))
            .or_else(|| self.values.get(&(token.as_ptr() as usize)))
    }
    pub fn get_ref(&self, name: &str, address: usize) -> Option<&i64> {
        self.legacy
            .as_ref()
            .and_then(|m| m.get(name))
            .or_else(|| self.values.get(&address))
    }
    pub fn array(&self, token: &str) -> Option<&Vec<Option<i64>>> {
        self.arrays.get(&(token.as_ptr() as usize))
    }
}
#[derive(Clone, Debug)]
enum Use {
    ComputedRead,
    DirectCall,
    Method(String),
    Other,
}
#[derive(Clone)]
struct Reference {
    at: u32,
    scope: ScopeId,
    block: Option<NodeId>,
    usage: Use,
    argument: Option<u32>,
}
#[derive(Default)]
struct Binding {
    init_end: u32,
    init_scope: Option<ScopeId>,
    initial: Option<Value>,
    initialized: bool,
    function: bool,
    function_escapes: bool,
    hoisted: bool,
    empty_array: bool,
    redeclared: bool,
    writes: Vec<Span>,
    member_written: bool,
    global_var: bool,
    references: Vec<Reference>,
    non_computed_reads: Vec<u32>,
}
impl Binding {
    fn stable(&self) -> bool {
        self.initialized && !self.redeclared && self.writes.is_empty() && !self.member_written
    }
}
pub(super) struct Function {
    pub name: u32,
    pub params: Vec<u32>,
    pub begin: u32,
    pub end: u32,
}
pub(super) struct Initializer {
    pub symbol: SymbolId,
    pub binding: Span,
    pub expression: Span,
    pub simple: bool,
    pub callable: bool,
    pub seed_operator: bool,
}
#[derive(Default)]
pub(super) struct Analysis {
    base: usize,
    identifiers: HashMap<u32, (SymbolId, ScopeId)>,
    bindings: HashMap<SymbolId, Binding>,
    globals: HashMap<u32, String>,
    global_writes: HashSet<String>,
    global_members: HashSet<String>,
    scope_parents: HashMap<ScopeId, Option<ScopeId>>,
    subtract: HashSet<SymbolId>,
    pub functions: Vec<Function>,
    pub key_pairs: Vec<(SymbolId, bool)>,
    pub numbers: Numbers,
    pub initializers: Vec<Initializer>,
    pub unproven_key_writes: Vec<Span>,
    value_dependents: HashMap<SymbolId, Vec<SymbolId>>,
    pub string_arrays: Vec<(usize, Vec<String>)>,
    pub pool_owners: Vec<(SymbolId, usize)>,
    registered_arrays: HashSet<usize>,
    unary_helpers: HashSet<SymbolId>,
    poisoned: HashSet<u32>,
    poisoned_positions: Vec<u32>,
    reads: Vec<(u32, SymbolId)>,
    pub dynamic: bool,
    builtin_array_dirty: bool,
}
impl Analysis {
    pub fn parse(source: &str) -> Option<Self> {
        if source.len() > 8_000_000 {
            return None;
        }
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::script()).parse();
        if parsed.fatal_error || !parsed.diagnostics.is_empty() {
            return None;
        }
        let built = SemanticBuilder::new_compiler()
            .with_build_nodes(true)
            .build(&parsed.program);
        if !built.diagnostics.is_empty() {
            return None;
        }
        Builder::new(source, &built.semantic).finish()
    }
    pub fn offset(&self, token: &str) -> Option<u32> {
        u32::try_from((token.as_ptr() as usize).checked_sub(self.base)?).ok()
    }
    pub fn symbol(&self, token: &str) -> Option<SymbolId> {
        let at = self.offset(token)?;
        if self.poisoned.contains(&at) {
            return None;
        }
        self.identifiers.get(&at).map(|id| id.0)
    }
    pub fn same(&self, a: &str, b: &str) -> bool {
        self.symbol(a).is_some_and(|id| Some(id) == self.symbol(b))
    }
    fn descendant(&self, mut scope: ScopeId, ancestor: ScopeId) -> bool {
        loop {
            if scope == ancestor {
                return true;
            }
            let Some(Some(parent)) = self.scope_parents.get(&scope) else {
                return false;
            };
            scope = *parent;
        }
    }
    fn available_at(&self, id: SymbolId, at: u32, scope: ScopeId) -> bool {
        self.bindings.get(&id).is_some_and(|b| {
            b.initialized
                && (b.hoisted
                    || (b.init_end < at
                        && b.init_scope
                            .is_some_and(|parent| self.descendant(scope, parent))))
        })
    }
    pub fn available(&self, token: &str) -> bool {
        let Some(at) = self.offset(token) else {
            return false;
        };
        let Some(id) = self.symbol(token) else {
            return false;
        };
        self.identifiers
            .get(&at)
            .is_some_and(|(_, scope)| self.available_at(id, at, *scope))
    }
    pub fn available_before(&self, dependency: &str, call: &str) -> bool {
        let Some(id) = self.symbol(dependency) else {
            return false;
        };
        let Some(at) = self.offset(call) else {
            return false;
        };
        self.identifiers
            .get(&at)
            .is_some_and(|(_, scope)| self.available_at(id, at, *scope))
    }
    pub fn is_registered_array(&self, offset: usize) -> bool {
        self.registered_arrays.contains(&offset)
    }
    pub fn unary_seed(&self, token: &str) -> bool {
        token != "parseInt"
            && self
                .symbol(token)
                .is_some_and(|id| self.unary_helpers.contains(&id))
    }
    pub fn poisoned_in(&self, span: Span) -> bool {
        Self::has_indexed_read(&self.poisoned_positions, span)
    }
    /// Sorted positions of pool-derived reads, built once per pool set.
    pub fn derived_read_positions(&self, roots: impl IntoIterator<Item = SymbolId>) -> Vec<u32> {
        let derived = self.derived_from(roots);
        self.reads
            .iter()
            .filter_map(|(at, id)| derived.contains(id).then_some(*at))
            .collect()
    }
    pub fn has_indexed_read(positions: &[u32], span: Span) -> bool {
        let index = positions.partition_point(|at| *at < span.start);
        positions.get(index).is_some_and(|at| *at < span.end)
    }
    /// Conservative local value provenance, used only to reject weak fallback.
    /// A worklist visits each derived binding once, including cyclic graphs.
    pub fn derived_from(&self, roots: impl IntoIterator<Item = SymbolId>) -> HashSet<SymbolId> {
        let mut derived = HashSet::default();
        let mut pending = Vec::new();
        for id in roots {
            if derived.insert(id) {
                pending.push(id);
            }
        }
        while let Some(id) = pending.pop() {
            if let Some(dependents) = self.value_dependents.get(&id) {
                for next in dependents {
                    if derived.insert(*next) {
                        pending.push(*next);
                    }
                }
            }
        }
        derived
    }
    fn record_value_source(
        &mut self,
        id: SymbolId,
        span: Span,
        remaining: &mut usize,
    ) -> Option<()> {
        let start = self.reads.partition_point(|r| r.0 < span.start);
        let end = self.reads.partition_point(|r| r.0 < span.end);
        *remaining = remaining.checked_sub(end - start)?;
        for index in start..end {
            let dependency = self.reads[index].1;
            if dependency != id {
                self.value_dependents
                    .entry(dependency)
                    .or_default()
                    .push(id);
            }
        }
        Some(())
    }
    pub fn stable(&self, token: &str) -> bool {
        self.symbol(token)
            .and_then(|id| self.bindings.get(&id))
            .is_some_and(|b| b.stable() && !b.function_escapes)
    }
    pub fn pool_intact_symbol(&self, id: SymbolId) -> bool {
        self.bindings
            .get(&id)
            .is_some_and(|b| b.stable() && b.non_computed_reads.is_empty())
    }
    pub fn builtin(&self, token: &str, name: &str) -> bool {
        self.offset(token)
            .filter(|p| !self.poisoned.contains(p))
            .and_then(|p| self.globals.get(&p))
            .is_some_and(|n| n == name)
            && !self.global_writes.contains(name)
            && !self.global_members.contains(name)
    }
    pub fn subtract(&self, token: &str) -> bool {
        self.symbol(token)
            .is_some_and(|id| self.subtract.contains(&id))
            && self.stable(token)
    }
    pub fn builtin_methods_intact(&self) -> bool {
        !self.builtin_array_dirty
            && !["String", "Array", "Function"]
                .iter()
                .any(|n| self.global_writes.contains(*n) || self.global_members.contains(*n))
    }
    pub fn dependency(&self, token: &str, begin: u32, end: u32, stack: bool) -> bool {
        let Some(id) = self.symbol(token) else {
            return false;
        };
        let Some(b) = self.bindings.get(&id) else {
            return false;
        };
        if !b.stable() || (stack && !b.empty_array) {
            return false;
        }
        if !stack {
            return b.non_computed_reads.first().is_none_or(|at| *at >= begin)
                && b.non_computed_reads.last().is_none_or(|at| *at < end);
        }
        let first = b.references.partition_point(|r| r.at < begin);
        let last = b.references.partition_point(|r| r.at < end);
        if b.references.len() - (last - first) != 1 {
            return false;
        }
        let outside = if first > 0 {
            &b.references[0]
        } else {
            &b.references[last]
        };
        matches!(&outside.usage,Use::Method(method) if method=="push")
            && b.init_scope == Some(outside.scope)
            && b.init_end < outside.at
            && outside.block.is_some()
    }
    pub fn numeric_data(&self, token: &str, begin: u32, end: u32) -> bool {
        self.symbol(token)
            .and_then(|id| self.bindings.get(&id))
            .is_some_and(|b| {
                b.initialized
                    && !b.redeclared
                    && matches!(b.initial, Some(Value::Number(_)))
                    && b.init_end < begin
                    && !b.member_written
                    && b.writes.iter().all(|w| begin <= w.start && w.end <= end)
            })
    }
    pub fn push_seed(&self, stack: &str, callee: &str) -> Option<i64> {
        if !self.same_push_block(stack, callee) {
            return None;
        }
        let b = self.bindings.get(&self.symbol(stack)?)?;
        let push = b
            .references
            .iter()
            .find(|r| matches!(&r.usage,Use::Method(m) if m=="push"))?;
        self.numbers
            .arguments
            .get(&(self.base + push.argument? as usize))
            .copied()
    }
    pub fn same_push_block(&self, stack: &str, call: &str) -> bool {
        let Some(id) = self.symbol(stack) else {
            return false;
        };
        let Some(at) = self.offset(call) else {
            return false;
        };
        let push = self.bindings[&id]
            .references
            .iter()
            .find(|r| matches!(&r.usage,Use::Method(m) if m=="push"));
        let call_block = self
            .symbol(call)
            .and_then(|callee| self.bindings.get(&callee))
            .and_then(|b| {
                let index = b.references.partition_point(|r| r.at < at);
                b.references.get(index).filter(|r| r.at == at)
            });
        matches!((push,call_block),(Some(a),Some(b)) if a.block.is_some() && a.block==b.block && a.at<b.at)
    }
}

struct Builder<'s, 'a> {
    source: &'s str,
    semantic: &'s Semantic<'a>,
    out: Analysis,
    inits: HashMap<SymbolId, &'a Expression<'a>>,
    helpers: HashMap<SymbolId, (&'static str, SymbolId)>,
    memo: HashMap<SymbolId, Option<Value>>,
    visiting: HashSet<SymbolId>,
}
impl<'s, 'a> Builder<'s, 'a> {
    fn new(source: &'s str, semantic: &'s Semantic<'a>) -> Self {
        Self {
            source,
            semantic,
            out: Analysis {
                base: source.as_ptr() as usize,
                identifiers: HashMap::default(),
                bindings: HashMap::default(),
                globals: HashMap::default(),
                global_writes: HashSet::default(),
                global_members: HashSet::default(),
                scope_parents: HashMap::default(),
                subtract: HashSet::default(),
                functions: Vec::new(),
                key_pairs: Vec::new(),
                numbers: Numbers::default(),
                initializers: Vec::new(),
                unproven_key_writes: Vec::new(),
                value_dependents: HashMap::default(),
                string_arrays: Vec::new(),
                pool_owners: Vec::new(),
                registered_arrays: HashSet::default(),
                unary_helpers: HashSet::default(),
                poisoned: HashSet::default(),
                poisoned_positions: Vec::new(),
                reads: Vec::new(),
                dynamic: false,
                builtin_array_dirty: false,
            },
            inits: HashMap::default(),
            helpers: HashMap::default(),
            memo: HashMap::default(),
            visiting: HashSet::default(),
        }
    }
    fn block(&self, id: NodeId) -> Option<NodeId> {
        let member = self.parent(id);
        let call = if matches!(
            self.semantic.nodes().kind(member),
            AstKind::CallExpression(_)
        ) {
            member
        } else {
            self.parent(member)
        };
        if !matches!(self.semantic.nodes().kind(call), AstKind::CallExpression(_)) {
            return None;
        }
        let mut statement = self.parent(call);
        if let AstKind::VariableDeclarator(d) = self.semantic.nodes().kind(statement) {
            if d.init.as_ref()?.get_inner_expression().span()
                != self.semantic.nodes().kind(call).span()
            {
                return None;
            }
            statement = self.semantic.nodes().parent_id(statement);
            if !matches!(
                self.semantic.nodes().kind(statement),
                AstKind::VariableDeclaration(_)
            ) {
                return None;
            }
        } else if !matches!(
            self.semantic.nodes().kind(statement),
            AstKind::ExpressionStatement(_)
        ) {
            return None;
        }
        let container = self.semantic.nodes().parent_id(statement);
        matches!(
            self.semantic.nodes().kind(container),
            AstKind::Program(_) | AstKind::BlockStatement(_) | AstKind::FunctionBody(_)
        )
        .then_some(container)
    }
    fn direct_initializer(&self, id: NodeId) -> bool {
        let declaration = self.semantic.nodes().parent_id(id);
        let container = self.semantic.nodes().parent_id(declaration);
        matches!(
            self.semantic.nodes().kind(declaration),
            AstKind::VariableDeclaration(_)
        ) && matches!(
            self.semantic.nodes().kind(container),
            AstKind::Program(_) | AstKind::BlockStatement(_) | AstKind::FunctionBody(_)
        )
    }
    fn pool_array(e: &Expression<'a>) -> Option<usize> {
        match e.get_inner_expression() {
            Expression::ArrayExpression(a) => Some(a.span.start as usize),
            Expression::CallExpression(c)
                if matches!(c.callee.get_inner_expression(), Expression::Identifier(_)) =>
            {
                let Expression::ArrayExpression(wrapper) =
                    c.arguments.get(1)?.as_expression()?.get_inner_expression()
                else {
                    return None;
                };
                let Expression::ArrayExpression(pool) = wrapper
                    .elements
                    .first()?
                    .as_expression()?
                    .get_inner_expression()
                else {
                    return None;
                };
                Some(pool.span.start as usize)
            }
            _ => None,
        }
    }
    fn parent(&self, mut id: NodeId) -> NodeId {
        loop {
            let p = self.semantic.nodes().parent_id(id);
            if matches!(
                self.semantic.nodes().kind(p),
                AstKind::ParenthesizedExpression(_)
            ) {
                id = p;
            } else {
                return p;
            }
        }
    }
    fn member_written(&self, id: NodeId) -> bool {
        let mut child = id;
        for parent in self.semantic.nodes().ancestor_ids(id) {
            match self.semantic.nodes().kind(parent) {
                AstKind::ParenthesizedExpression(_)
                | AstKind::ArrayAssignmentTarget(_)
                | AstKind::ObjectAssignmentTarget(_)
                | AstKind::AssignmentTargetRest(_)
                | AstKind::AssignmentTargetWithDefault(_)
                | AstKind::AssignmentTargetPropertyProperty(_) => {
                    child = parent;
                    continue;
                }
                AstKind::AssignmentExpression(a) => {
                    return a.left.span().start <= self.semantic.nodes().kind(child).span().start
                        && self.semantic.nodes().kind(child).span().end <= a.left.span().end;
                }
                AstKind::UpdateExpression(_) => return true,
                AstKind::UnaryExpression(u) => return u.operator.as_str() == "delete",
                AstKind::ForInStatement(f) => {
                    return self.semantic.nodes().kind(child).span().end <= f.left.span().end;
                }
                AstKind::ForOfStatement(f) => {
                    return self.semantic.nodes().kind(child).span().end <= f.left.span().end;
                }
                _ => return false,
            }
        }
        false
    }
    fn root<'b>(&self, mut e: &'b Expression<'a>) -> &'b Expression<'a> {
        loop {
            e = e.get_inner_expression();
            if let Some(m) = e.as_member_expression() {
                e = m.object();
            } else {
                return e;
            }
        }
    }
    fn usage(&self, id: NodeId) -> Use {
        let p = self.parent(id);
        let kind = self.semantic.nodes().kind(p);
        if let Some(m) = kind.as_member_expression_kind() {
            if m.object().get_inner_expression().span() != self.semantic.nodes().kind(id).span() {
                return Use::Other;
            }
            let next = self.parent(p);
            if let AstKind::CallExpression(c) = self.semantic.nodes().kind(next) {
                if c.callee.get_inner_expression().span() == kind.span() {
                    return m
                        .static_property_name()
                        .map_or(Use::Other, |n| Use::Method(n.display().to_string()));
                }
            }
            if matches!(kind, AstKind::ComputedMemberExpression(_)) && !self.member_written(p) {
                return Use::ComputedRead;
            }
            return Use::Other;
        }
        if let AstKind::CallExpression(c) = kind {
            if c.callee.get_inner_expression().span() == self.semantic.nodes().kind(id).span() {
                return Use::DirectCall;
            }
        }
        Use::Other
    }
    fn method_argument(&self, id: NodeId) -> Option<u32> {
        let member = self.parent(id);
        let call = self.parent(member);
        if let AstKind::CallExpression(c) = self.semantic.nodes().kind(call) {
            if c.arguments.len() == 1 {
                return c.arguments[0].as_expression().map(|a| a.span().start);
            }
        }
        None
    }
    fn function_binding(
        &self,
        id: NodeId,
        f: &'a oxc_ast::ast::Function<'a>,
    ) -> Option<(SymbolId, u32)> {
        if f.r#type == FunctionType::FunctionDeclaration {
            let binding = f.id.as_ref()?;
            return Some((binding.symbol_id.get()?, binding.span.start));
        }
        match self.semantic.nodes().kind(self.parent(id)) {
            AstKind::VariableDeclarator(d) => {
                if let BindingPattern::BindingIdentifier(b) = &d.id {
                    Some((b.symbol_id.get()?, b.span.start))
                } else {
                    None
                }
            }
            _ => None,
        }
    }
    fn finish(mut self) -> Option<Analysis> {
        let scoping = self.semantic.scoping();
        let mut annex: HashMap<ScopeId, HashSet<String>> = HashMap::default();
        for scope in scoping.scope_descendants_from_root() {
            self.out
                .scope_parents
                .insert(scope, scoping.scope_parent_id(scope));
        }
        for node in self.semantic.nodes().iter() {
            if let AstKind::Function(f) = node.kind() {
                if f.r#type == FunctionType::FunctionDeclaration
                    && !scoping.scope_flags(node.scope_id()).is_var()
                    && !scoping.scope_flags(node.scope_id()).is_strict_mode()
                {
                    if let (Some(name), Some(scope)) = (
                        f.id.as_ref(),
                        scoping
                            .scope_ancestors(node.scope_id())
                            .find(|s| scoping.scope_flags(*s).is_var()),
                    ) {
                        annex
                            .entry(scope)
                            .or_default()
                            .insert(name.name.to_string());
                    }
                }
            }
        }
        for id in scoping.symbol_ids() {
            let global_var = scoping.symbol_scope_id(id) == scoping.root_scope_id()
                && (scoping.symbol_flags(id).is_function_scoped_declaration()
                    || scoping.symbol_flags(id).is_function());
            self.out.bindings.insert(
                id,
                Binding {
                    redeclared: !scoping.symbol_redeclarations(id).is_empty(),
                    global_var,
                    ..Binding::default()
                },
            );
        }
        for node in self.semantic.nodes().iter() {
            match node.kind() {
                AstKind::WithStatement(_) => self.out.dynamic = true,
                AstKind::BindingIdentifier(b) => {
                    if let Some(id) = b.symbol_id.get() {
                        self.out
                            .identifiers
                            .insert(b.span.start, (id, node.scope_id()));
                        for parent in self.semantic.nodes().ancestors(node.id()) {
                            if let AstKind::VariableDeclarator(d) = parent.kind() {
                                if d.id.span().start <= b.span.start
                                    && b.span.end <= d.id.span().end
                                {
                                    if let Some(init) = &d.init {
                                        self.out.initializers.push(Initializer{symbol:id,binding:b.span,expression:if matches!(d.id,BindingPattern::BindingIdentifier(_)){init.span()}else{d.span},simple:matches!(d.id,BindingPattern::BindingIdentifier(_)),callable:matches!(init.get_inner_expression(),Expression::FunctionExpression(_)|Expression::ArrowFunctionExpression(_)),seed_operator:matches!(init.get_inner_expression(),Expression::UnaryExpression(u) if matches!(u.operator.as_str(),"-"|"!"|"~"))});
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
                AstKind::VariableDeclarator(d) => {
                    if let BindingPattern::BindingIdentifier(b) = &d.id {
                        let id = b.symbol_id.get()?;
                        let direct = self.direct_initializer(node.id());
                        let fact = self.out.bindings.get_mut(&id)?;
                        if let Some(init) = &d.init {
                            fact.initialized = direct;
                            fact.init_scope = Some(node.scope_id());
                            fact.init_end = init.span().end;
                            fact.empty_array = matches!(init.get_inner_expression(),Expression::ArrayExpression(a) if a.elements.is_empty());
                            self.inits.insert(id, init);
                            if let Some(array) = Self::pool_array(init) {
                                self.out.pool_owners.push((id, array));
                            }
                        }
                    }
                }
                AstKind::AssignmentExpression(a) => {
                    if let AssignmentTarget::AssignmentTargetIdentifier(reference) = &a.left {
                        if let (Some(symbol), Some(array)) = (
                            reference
                                .reference_id
                                .get()
                                .and_then(|r| scoping.get_reference(r).symbol_id()),
                            Self::pool_array(&a.right),
                        ) {
                            self.out.pool_owners.push((symbol, array));
                        }
                    }
                }
                AstKind::Function(f) => {
                    let Some((id, name)) = self.function_binding(node.id(), f) else {
                        continue;
                    };
                    let fact = self.out.bindings.get_mut(&id)?;
                    fact.function = true;
                    fact.hoisted = f.r#type == FunctionType::FunctionDeclaration;
                    fact.initialized = true;
                    let Some(body) = &f.body else {
                        continue;
                    };
                    let params: Option<Vec<_>> = f
                        .params
                        .items
                        .iter()
                        .map(|p| {
                            if let BindingPattern::BindingIdentifier(b) = &p.pattern {
                                p.initializer.is_none().then_some(b.span.start)
                            } else {
                                None
                            }
                        })
                        .collect();
                    if let Some(params) =
                        params.filter(|_| f.params.rest.is_none() && !f.generator && !f.r#async)
                    {
                        self.out.functions.push(Function {
                            name,
                            params,
                            begin: body.span.start + 1,
                            end: body.span.end - 1,
                        });
                    }
                    if !f.r#async
                        && !f.generator
                        && body.statements.len() == 1
                        && f.params.items.len() == 1
                        && f.params.rest.is_none()
                    {
                        if let (
                            BindingPattern::BindingIdentifier(param),
                            Statement::ReturnStatement(ret),
                        ) = (&f.params.items[0].pattern, &body.statements[0])
                        {
                            if let Some(Expression::UnaryExpression(u)) = &ret.argument {
                                if let Expression::Identifier(input) =
                                    u.argument.get_inner_expression()
                                {
                                    if input
                                        .reference_id
                                        .get()
                                        .and_then(|r| scoping.get_reference(r).symbol_id())
                                        == param.symbol_id.get()
                                        && matches!(u.operator.as_str(), "-" | "!")
                                    {
                                        self.helpers.insert(
                                            id,
                                            (u.operator.as_str(), param.symbol_id.get()?),
                                        );
                                    }
                                }
                            }
                        }
                    }
                    if !f.r#async
                        && !f.generator
                        && body.statements.len() == 1
                        && f.params.items.len() == 2
                        && f.params.rest.is_none()
                    {
                        if let Statement::ReturnStatement(ret) = &body.statements[0] {
                            if let Some(Expression::BinaryExpression(b)) = &ret.argument {
                                let param_ids: Vec<_> = f
                                    .params
                                    .items
                                    .iter()
                                    .filter_map(|p| {
                                        if let BindingPattern::BindingIdentifier(b) = &p.pattern {
                                            b.symbol_id.get()
                                        } else {
                                            None
                                        }
                                    })
                                    .collect();
                                let ids: Option<Vec<_>> = [&b.left, &b.right]
                                    .iter()
                                    .map(|e| {
                                        if let Expression::Identifier(i) = e.get_inner_expression()
                                        {
                                            i.reference_id
                                                .get()
                                                .and_then(|r| scoping.get_reference(r).symbol_id())
                                        } else {
                                            None
                                        }
                                    })
                                    .collect();
                                if b.operator.as_str() == "-"
                                    && ids.as_ref() == Some(&param_ids)
                                    && param_ids.len() == 2
                                {
                                    self.out.subtract.insert(id);
                                }
                            }
                        }
                    }
                }
                AstKind::IdentifierReference(r) => {
                    let reference = scoping.get_reference(r.reference_id.get()?);
                    if !annex.is_empty() {
                        for scope in scoping.scope_ancestors(node.scope_id()) {
                            if annex
                                .get(&scope)
                                .is_some_and(|names| names.contains(r.name.as_str()))
                            {
                                self.out.poisoned.insert(r.span.start);
                                break;
                            }
                            if scoping.get_binding(scope, r.name).is_some() {
                                break;
                            }
                        }
                    }

                    if let Some(id) = reference.symbol_id() {
                        self.out
                            .identifiers
                            .insert(r.span.start, (id, node.scope_id()));
                        self.out.reads.push((r.span.start, id));
                        let usage = self.usage(node.id());
                        let block = self.block(node.id());
                        let argument = self.method_argument(node.id());
                        let fact = self.out.bindings.get_mut(&id)?;
                        if reference.is_write() {
                            fact.writes.push(r.span);
                        }
                        fact.references.push(Reference {
                            at: r.span.start,
                            scope: node.scope_id(),
                            block,
                            usage,
                            argument,
                        });
                    } else {
                        self.out.globals.insert(r.span.start, r.name.to_string());
                        if matches!(r.name.as_str(), "String" | "Array" | "Function")
                            && matches!(self.usage(node.id()), Use::Other)
                            && matches!(
                                self.semantic.nodes().kind(self.parent(node.id())),
                                AstKind::CallExpression(_)
                                    | AstKind::VariableDeclarator(_)
                                    | AstKind::AssignmentExpression(_)
                                    | AstKind::ReturnStatement(_)
                                    | AstKind::ArrayExpression(_)
                                    | AstKind::ObjectProperty(_)
                            )
                        {
                            self.out.global_members.insert(r.name.to_string());
                        }
                        if reference.is_write() {
                            self.out.global_writes.insert(r.name.to_string());
                        }
                    }
                }
                _ => {}
            }
        }
        for node in self.semantic.nodes().iter() {
            let Some(member) = node.kind().as_member_expression_kind() else {
                continue;
            };
            let root = self.root(member.object());
            if member
                .static_property_name()
                .is_some_and(|p| matches!(p.as_str(), Some("prototype" | "__proto__")))
            {
                match root {
                    Expression::Identifier(r)
                        if r.reference_id
                            .get()
                            .and_then(|id| scoping.get_reference(id).symbol_id())
                            .is_none()
                            && matches!(r.name.as_str(), "Array" | "Function" | "String") =>
                    {
                        self.out.global_members.insert(r.name.to_string());
                    }
                    Expression::ArrayExpression(_) => self.out.builtin_array_dirty = true,
                    _ => {}
                }
            }
            if !self.member_written(node.id()) {
                continue;
            }

            match root {
                Expression::Identifier(r) => {
                    let symbol = r
                        .reference_id
                        .get()
                        .and_then(|id| scoping.get_reference(id).symbol_id());
                    if let Some(id) = symbol {
                        self.out.bindings.get_mut(&id)?.member_written = true;
                    } else {
                        let name = r.name.as_str();
                        self.out.global_members.insert(name.to_owned());
                        if matches!(name, "globalThis" | "window" | "self") {
                            if let Some(property) = member.static_property_name() {
                                self.out
                                    .global_writes
                                    .insert(property.display().to_string());
                            }
                        }
                    }
                }
                Expression::ThisExpression(_) => {
                    if let Some(property) = member.static_property_name() {
                        self.out
                            .global_writes
                            .insert(property.display().to_string());
                    }
                }
                Expression::ArrayExpression(_) => self.out.builtin_array_dirty = true,
                _ => {}
            }
        }
        for id in scoping.symbol_ids() {
            let fact = self.out.bindings.get_mut(&id)?;
            if fact.global_var && self.out.global_writes.contains(scoping.symbol_name(id)) {
                fact.member_written = true;
            }
        }
        for fact in self.out.bindings.values_mut() {
            fact.references
                .sort_unstable_by_key(|reference| reference.at);
            fact.non_computed_reads = fact
                .references
                .iter()
                .filter(|r| !matches!(r.usage, Use::ComputedRead))
                .map(|r| r.at)
                .collect();

            fact.function_escapes = fact.function
                && fact.references.iter().any(|r| {
                    !matches!(r.usage, Use::DirectCall)
                        && !matches!(&r.usage,Use::Method(name) if name=="apply")
                });
        }
        self.out.reads.sort_unstable_by_key(|r| r.0);
        self.out.poisoned_positions = self.out.poisoned.iter().copied().collect();
        self.out.poisoned_positions.sort_unstable();
        self.out.unary_helpers.extend(self.helpers.keys().copied());
        let inits: Vec<_> = self.inits.iter().map(|(id, e)| (*id, *e)).collect();
        for (id, e) in inits {
            let v = self.expr(e, 0);
            self.out.bindings.get_mut(&id)?.initial = v;
        }
        for node in self.semantic.nodes().iter() {
            let value = if matches!(
                node.kind(),
                AstKind::IdentifierReference(_) | AstKind::NumericLiteral(_)
            ) {
                self.kind_value(node.kind(), 0)
            } else {
                None
            };
            if let Some(Value::Number(n)) = value.filter(|_| {
                matches!(
                    node.kind(),
                    AstKind::IdentifierReference(_) | AstKind::NumericLiteral(_)
                )
            }) {
                self.out
                    .numbers
                    .values
                    .insert(self.out.base + node.span().start as usize, n);
            }
            if let AstKind::CallExpression(c) = node.kind() {
                if c.arguments.len() == 1 {
                    if let Some(arg) = c.arguments[0].as_expression() {
                        if let Some(Value::Number(n)) = self.expr(arg, 0) {
                            self.out
                                .numbers
                                .arguments
                                .insert(self.out.base + arg.span().start as usize, n);
                        }
                    }
                }
            }
            if let AstKind::ArrayExpression(a) = node.kind() {
                let strings: Option<Vec<String>> = a
                    .elements
                    .iter()
                    .map(|e| match e.as_expression()?.get_inner_expression() {
                        Expression::StringLiteral(s) => s.value.as_str().map(str::to_owned),
                        _ => None,
                    })
                    .collect();
                if let Some(strings) = strings {
                    self.out
                        .string_arrays
                        .push((a.span.start as usize, strings));
                }
                let parent = self.parent(node.id());
                if let AstKind::CallExpression(c) = self.semantic.nodes().kind(parent) {
                    if matches!(
                        c.callee.get_inner_expression(),
                        Expression::FunctionExpression(_)
                    ) && c
                        .arguments
                        .first()
                        .and_then(Argument::as_expression)
                        .is_some_and(|e| e.get_inner_expression().span() == a.span)
                    {
                        self.out.registered_arrays.insert(a.span.start as usize);
                    }
                }

                // Retain every possible pair value as rejection evidence,
                // including inline, wrapped and spread expressions. Stronger
                // proven extraction strategies keep their existing precedence.
                if a.elements.len() == 2 {
                    self.out.unproven_key_writes.push(a.elements[1].span());
                }

                if a.elements.len() == 2
                    && a.elements[0].as_expression().is_some_and(|e| {
                        matches!(
                            e.get_inner_expression(),
                            Expression::Identifier(_) | Expression::NumericLiteral(_)
                        )
                    })
                {
                    if let Some(Expression::Identifier(reference)) = a.elements[1]
                        .as_expression()
                        .map(Expression::get_inner_expression)
                    {
                        if let Some(symbol) = reference
                            .reference_id
                            .get()
                            .and_then(|r| scoping.get_reference(r).symbol_id())
                        {
                            self.out.key_pairs.push((
                                symbol,
                                self.out.available(
                                    &self.source[reference.span.start as usize
                                        ..reference.span.end as usize],
                                ),
                            ));
                        }
                    }
                }

                if a.elements.len() > 100_000 {
                    return None;
                }
                let numbers = a
                    .elements
                    .iter()
                    .map(|e| {
                        e.as_expression().and_then(|e| match self.expr(e, 0) {
                            Some(Value::Number(n)) => Some(n),
                            _ => None,
                        })
                    })
                    .collect();
                self.out
                    .numbers
                    .arrays
                    .insert(self.out.base + a.span.start as usize, numbers);
            }
        }
        let key_ids: HashSet<_> = self.out.key_pairs.iter().map(|(id, _)| *id).collect();
        let mut global_pairs = HashSet::default();
        for node in self.semantic.nodes().iter() {
            if let AstKind::ArrayExpression(a) = node.kind() {
                if a.elements.len() != 2
                    || !a.elements[0].as_expression().is_some_and(|e| {
                        matches!(
                            e.get_inner_expression(),
                            Expression::Identifier(_) | Expression::NumericLiteral(_)
                        )
                    })
                {
                    continue;
                }
                if let Some(Expression::Identifier(r)) = a.elements[1]
                    .as_expression()
                    .map(Expression::get_inner_expression)
                {
                    if scoping
                        .get_reference(r.reference_id.get()?)
                        .symbol_id()
                        .is_none()
                    {
                        global_pairs.insert(r.name.as_str());
                    }
                }
            }
        }
        // Record local provenance for every binding, not just key-pair names.
        // Rejection evidence may flow through intermediate values or aliases.
        // Bound total recorded reference work to avoid nested-span blowups.
        let mut dependency_work = 2_000_000usize;
        for node in self.semantic.nodes().iter() {
            let (symbol, target, is_key) = match node.kind() {
                AstKind::BindingIdentifier(b) => {
                    let symbol = b.symbol_id.get()?;
                    (Some(symbol), b.span, key_ids.contains(&symbol))
                }
                AstKind::IdentifierReference(r) => {
                    let reference = scoping.get_reference(r.reference_id.get()?);
                    if !reference.is_write() {
                        continue;
                    }
                    let symbol = reference.symbol_id();
                    let is_key = symbol.map_or_else(
                        || global_pairs.contains(r.name.as_str()),
                        |id| key_ids.contains(&id),
                    );
                    if symbol.is_none() && !is_key {
                        continue;
                    }
                    (symbol, r.span, is_key)
                }
                _ => continue,
            };
            let contains = |span: Span| span.start <= target.start && target.end <= span.end;
            let mut sources = Vec::new();
            for parent in self.semantic.nodes().ancestors(node.id()) {
                match parent.kind() {
                    AstKind::VariableDeclarator(d) if contains(d.id.span()) => {
                        let simple = matches!(d.id, BindingPattern::BindingIdentifier(_));
                        if !simple {
                            sources.push(d.span);
                            if is_key {
                                self.out.unproven_key_writes.push(d.span);
                            }
                        }
                        if let Some(init) = &d.init {
                            if simple
                                && !matches!(
                                    init.get_inner_expression(),
                                    Expression::FunctionExpression(_)
                                        | Expression::ArrowFunctionExpression(_)
                                )
                            {
                                sources.push(init.span());
                            }
                            break;
                        }
                    }
                    AstKind::AssignmentExpression(a) => {
                        if contains(a.left.span()) {
                            let span = if matches!(
                                a.left,
                                AssignmentTarget::AssignmentTargetIdentifier(_)
                            ) {
                                a.right.span()
                            } else {
                                a.span
                            };
                            sources.push(span);
                            if is_key {
                                self.out.unproven_key_writes.push(span);
                            }
                        }
                        break;
                    }
                    AstKind::ForInStatement(f) if contains(f.left.span()) => {
                        let span = Span::new(f.left.span().start, f.right.span().end);
                        sources.push(span);
                        if is_key {
                            self.out.unproven_key_writes.push(span);
                        }
                        break;
                    }
                    AstKind::ForOfStatement(f) if contains(f.left.span()) => {
                        let span = Span::new(f.left.span().start, f.right.span().end);
                        sources.push(span);
                        if is_key {
                            self.out.unproven_key_writes.push(span);
                        }
                        break;
                    }
                    AstKind::FormalParameter(p) if contains(p.pattern.span()) => {
                        sources.push(p.span);
                        if is_key {
                            self.out.unproven_key_writes.push(p.span);
                        }
                        break;
                    }
                    AstKind::Function(_) | AstKind::ArrowFunctionExpression(_) => break,
                    _ => {}
                }
            }
            if let Some(symbol) = symbol {
                for span in sources {
                    self.out
                        .record_value_source(symbol, span, &mut dependency_work)?;
                }
            }
        }
        Some(self.out)
    }
    fn symbol_value(&mut self, id: SymbolId, depth: usize) -> Option<Value> {
        if depth > 32 {
            return None;
        }
        if let Some(v) = self.memo.get(&id) {
            return *v;
        }
        if !self.out.bindings.get(&id)?.stable() || !self.visiting.insert(id) {
            return None;
        }
        let value = self
            .inits
            .get(&id)
            .copied()
            .and_then(|e| self.expr(e, depth + 1));
        self.visiting.remove(&id);
        self.memo.insert(id, value);
        value
    }
    fn checked(n: i64) -> Option<Value> {
        (n.unsigned_abs() <= 9_007_199_254_740_991).then_some(Value::Number(n))
    }
    fn unary(op: &str, a: Value) -> Option<Value> {
        match op {
            "!" => Some(Value::Boolean(!a.truth())),
            "+" => Self::checked(a.number()?),
            "-" => Self::checked(a.number()?.checked_neg()?),
            _ => None,
        }
    }
    fn unary_value(&self, op: &str, a: Value) -> Option<Value> {
        if op != "!" && matches!(a, Value::EmptyArray) && !self.out.builtin_methods_intact() {
            return None;
        }
        Self::unary(op, a)
    }
    fn reference_value(&mut self, i: &IdentifierReference<'a>, depth: usize) -> Option<Value> {
        let id = i
            .reference_id
            .get()
            .and_then(|r| self.semantic.scoping().get_reference(r).symbol_id())?;
        if self.out.poisoned.contains(&i.span.start) {
            return None;
        }
        let scope = self.out.identifiers.get(&i.span.start)?.1;
        if !self.out.available_at(id, i.span.start, scope) {
            return None;
        }
        self.symbol_value(id, depth)
    }
    fn expr(&mut self, e: &Expression<'a>, depth: usize) -> Option<Value> {
        if depth > 32 {
            return None;
        }
        match e {
            Expression::Identifier(i) => self.reference_value(i, depth + 1),
            Expression::NumericLiteral(n) => self.literal(n.span, n.value),
            Expression::BooleanLiteral(b) => Some(Value::Boolean(b.value)),
            Expression::ArrayExpression(a) if a.elements.is_empty() => Some(Value::EmptyArray),
            Expression::ObjectExpression(o) if o.properties.is_empty() => Some(Value::Object),
            Expression::UnaryExpression(u) => {
                let a = self.expr(&u.argument, depth + 1)?;
                self.unary_value(u.operator.as_str(), a)
            }
            Expression::BinaryExpression(b) => {
                let a = self.expr(&b.left, depth + 1)?;
                let c = self.expr(&b.right, depth + 1)?;
                self.binary(b.operator.as_str(), a, c)
            }
            Expression::ParenthesizedExpression(p) => self.expr(&p.expression, depth + 1),
            Expression::CallExpression(c) => self.call_value(c, depth + 1),
            _ => None,
        }
    }
    fn kind_value(&mut self, k: AstKind<'a>, depth: usize) -> Option<Value> {
        match k {
            AstKind::IdentifierReference(i) => self.reference_value(i, depth + 1),
            AstKind::NumericLiteral(n) => self.literal(n.span, n.value),
            AstKind::UnaryExpression(u) => {
                let v = self.expr(&u.argument, depth + 1)?;
                self.unary_value(u.operator.as_str(), v)
            }
            AstKind::BinaryExpression(b) => {
                let a = self.expr(&b.left, depth + 1)?;
                let c = self.expr(&b.right, depth + 1)?;
                self.binary(b.operator.as_str(), a, c)
            }
            AstKind::ParenthesizedExpression(p) => self.expr(&p.expression, depth + 1),
            AstKind::CallExpression(c) => self.call_value(c, depth + 1),
            _ => None,
        }
    }
    fn call_value(&mut self, c: &CallExpression<'a>, depth: usize) -> Option<Value> {
        if c.optional || c.arguments.len() != 1 {
            return None;
        }
        let Expression::Identifier(i) = c.callee.get_inner_expression() else {
            return None;
        };
        let id = i
            .reference_id
            .get()
            .and_then(|r| self.semantic.scoping().get_reference(r).symbol_id())?;
        if self.out.poisoned.contains(&i.span.start) {
            return None;
        }
        let scope = self.out.identifiers.get(&i.span.start)?.1;
        if !self.out.bindings.get(&id)?.stable() || !self.out.available_at(id, c.span.start, scope)
        {
            return None;
        }
        let (op, _) = *self.helpers.get(&id)?;
        let arg = c.arguments[0].as_expression()?;
        let a = self.expr(arg, depth + 1)?;
        self.unary_value(op, a)
    }
    fn literal(&self, span: Span, value: f64) -> Option<Value> {
        let raw = &self.source[span.start as usize..span.end as usize];
        if raw.len() > 1 && raw.starts_with('0')
            || !raw.bytes().all(|b| b.is_ascii_digit())
            || value.fract() != 0.0
            || value.abs() > 9_007_199_254_740_991.0
        {
            return None;
        }
        Self::checked(value as i64)
    }
    fn binary(&self, op: &str, a: Value, b: Value) -> Option<Value> {
        if (matches!(a, Value::EmptyArray) || matches!(b, Value::EmptyArray))
            && !self.out.builtin_methods_intact()
        {
            return None;
        }
        if op == "+"
            && (matches!(a, Value::EmptyArray | Value::Object)
                || matches!(b, Value::EmptyArray | Value::Object))
        {
            return None;
        }
        let a = a.number()?;
        let b = b.number()?;
        Self::checked(match op {
            "+" => a.checked_add(b)?,
            "-" => a.checked_sub(b)?,
            "*" => a.checked_mul(b)?,
            "/" if b != 0 && a.checked_rem(b)? == 0 => a.checked_div(b)?,
            "%" => a.checked_rem(b)?,
            _ => return None,
        })
    }
}
