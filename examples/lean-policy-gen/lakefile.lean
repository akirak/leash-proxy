import Lake
open Lake DSL

/-- Example that generates a leash-proxy Cedar policy set in the Cedar JSON
(EST) format from a small typed policy DSL (see `Policies.lean`). -/
package «lean-policy-gen»

lean_lib Policies

@[default_target]
lean_exe gen where
  root := `Main
