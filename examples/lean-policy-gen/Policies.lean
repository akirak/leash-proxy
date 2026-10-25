/-
# Cedar policy JSON (EST) generation for leash-proxy

This module defines a small, EST-shaped policy AST: a Lean mirror of the subset
of the Cedar policy JSON format that leash-proxy policies need, with a
serializer into `Lean.Json`. The emitted policy set is accepted directly by
leash-proxy (`CedarEngine::from_files`) and by the official Cedar tooling.

The types are intentionally decoupled from the Cedar formal specification;
they only guarantee that the output is *well-formed JSON in the EST shape*,
not that it matches a particular schema. Always run the result through the
proxy's fail-closed load (or `cedar validate`) to check it against your schema.
-/
import Lean
open Lean (Json)
open Lean.Json

/-! ## Expressions

Cedar expressions, mirroring the EST (JSON) encoding of the Cedar expression
tree: operators are keyed by their Cedar source syntax (`"=="`, `"&&"`, ...)
and literals are wrapped in `Value` / `Var` objects. -/

/-- Cedar expressions in the EST (JSON) encoding. -/
inductive Expr where
  /-- One of the four request variables: `principal`, `action`, `resource`, `context`. -/
  | var (name : String)
  /-- A string literal. -/
  | str (s : String)
  /-- A boolean literal. -/
  | bool (b : Bool)
  /-- An integer literal. -/
  | long (n : Int)
  /-- An entity literal, e.g. `Host::"pypi.org"`. -/
  | entity (ty id : String)
  /-- `l == r` -/
  | eq (l r : Expr)
  /-- `l != r` -/
  | ne (l r : Expr)
  /-- `l && r` -/
  | and (l r : Expr)
  /-- `l || r` -/
  | or (l r : Expr)
  /-- `!e` -/
  | not (e : Expr)
  /-- `l in r` -/
  | mem (l r : Expr)
  /-- `l < r` -/
  | lt (l r : Expr)
  /-- `l <= r` -/
  | le (l r : Expr)
  /-- `l > r` -/
  | gt (l r : Expr)
  /-- `l >= r` -/
  | ge (l r : Expr)
  /-- `e.attr` -/
  | attr (e : Expr) (a : String)
  /-- `e has attr` -/
  | has (e : Expr) (a : String)
  /-- `e like pattern`, where `pattern` uses `*` as the wildcard. -/
  | like (e : Expr) (pattern : String)

/-- A binary operator node keyed by its Cedar source syntax. -/
def binop (op : String) (l r : Json) : Json :=
  Json.mkObj [(op, Json.mkObj [("left", l), ("right", r)])]

/-- A unary operator node keyed by its Cedar source syntax. -/
def unop (op : String) (arg : Json) : Json :=
  Json.mkObj [(op, Json.mkObj [("arg", arg)])]

/-- Encode a `like` pattern into the EST element list: literals and `*`
wildcards. Each wildcard-free character is its own literal element, matching
the canonical serialization produced by the Cedar tooling. -/
def patternElems (pattern : String) : Json :=
  let elems := pattern.toList.map fun
    | '*' => Json.str "Wildcard"
    | c   => Json.mkObj [("Literal", Json.str (String.singleton c))]
  Json.arr (List.toArray elems)

def Expr.toJson : Expr → Json
  | .var n      => Json.mkObj [("Var", Json.str n)]
  | .str s      => Json.mkObj [("Value", Json.str s)]
  | .bool b     => Json.mkObj [("Value", Json.bool b)]
  | .long n     => Json.mkObj [("Value", (n : Json))]
  | .entity t i => Json.mkObj
      [("Value", Json.mkObj [("__entity", Json.mkObj [("type", Json.str t), ("id", Json.str i)])])]
  | .eq l r     => binop "==" l.toJson r.toJson
  | .ne l r     => binop "!=" l.toJson r.toJson
  | .and l r    => binop "&&" l.toJson r.toJson
  | .or l r     => binop "||" l.toJson r.toJson
  | .not e      => unop "!" e.toJson
  | .mem l r    => binop "in" l.toJson r.toJson
  | .lt l r     => binop "<" l.toJson r.toJson
  | .le l r     => binop "<=" l.toJson r.toJson
  | .gt l r     => binop ">" l.toJson r.toJson
  | .ge l r     => binop ">=" l.toJson r.toJson
  | .attr e a   => Json.mkObj [(".", Json.mkObj [("left", e.toJson), ("attr", Json.str a)])]
  | .has e a    => Json.mkObj [("has", Json.mkObj [("left", e.toJson), ("attr", Json.str a)])]
  | .like e p   => Json.mkObj
      [("like", Json.mkObj [("left", e.toJson), ("pattern", patternElems p)])]

/-! ## Scope constraints -/

/-- A principal/action/resource scope constraint in the EST encoding. -/
inductive Scope where
  /-- No constraint (e.g. `principal,`). -/
  | any
  /-- `== Entity::"id"`. -/
  | eq (ty id : String)
  /-- `in Entity::"id"`. -/
  | mem (ty id : String)

def Scope.toJson : Scope → Json
  | .any     => Json.mkObj [("op", Json.str "All")]
  | .eq t i  => Json.mkObj [("op", Json.str "=="), ("entity", entityJson t i)]
  | .mem t i => Json.mkObj [("op", Json.str "in"), ("entity", entityJson t i)]
where
  entityJson (t i : String) : Json :=
    Json.mkObj [("type", Json.str t), ("id", Json.str i)]

/-! ## Policies -/

/-- A Cedar policy in the EST encoding: an effect, scope constraints, and
`when`/`unless` condition bodies. -/
structure Policy where
  /-- Policy id used in the set and in audit records. -/
  id : String
  /-- `"permit"` or `"forbid"`. -/
  effect : String := "permit"
  principal : Scope := .any
  action : Scope := .any
  resource : Scope := .any
  whens : List Expr := []
  unlesses : List Expr := []

def Policy.toJson (p : Policy) : Json :=
  let conds := p.whens.map (fun e => clause "when" e)
    ++ p.unlesses.map (fun e => clause "unless" e)
  Json.mkObj
    [ ("effect", Json.str p.effect)
    , ("principal", p.principal.toJson)
    , ("action", p.action.toJson)
    , ("resource", p.resource.toJson)
    , ("conditions", Json.arr (List.toArray conds)) ]
where
  clause (kind : String) (e : Expr) : Json :=
    Json.mkObj [("kind", Json.str kind), ("body", e.toJson)]

/-- A whole Cedar policy set in the EST encoding (static policies only:
`templates` and `templateLinks` are emitted empty).

 All three fields are mandatory in Cedar's JSON format, even when empty. -/
def policySet (ps : List Policy) : Json :=
  Json.mkObj
    [ ("staticPolicies", Json.mkObj (ps.map fun p => (p.id, p.toJson)))
    , ("templates", Json.mkObj [])
    , ("templateLinks", Json.arr #[]) ]

/-! ## Ergonomics

Infix operators, the four request variables, and leash-proxy's entity types
(`Agent`, `Host`, `Action`). These helpers are where a real deployment would
grow its policy vocabulary: factoring them out is the main win of authoring
policies in Lean rather than raw Cedar text. -/

infixr:35 " &&& " => Expr.and
infixr:30 " ||| " => Expr.or
infix:50  " === " => Expr.eq
infix:50  " !== " => Expr.ne

def principal : Expr := .var "principal"
def action : Expr := .var "action"
def resource : Expr := .var "resource"
def context : Expr := .var "context"

def agent (id : String) : Expr := .entity "Agent" id
def host (id : String) : Expr := .entity "Host" id
def httpGet : Scope := .eq "Action" "http_get"
