inductive CoherenceState : Type :=
  | Shared : CoherenceState
  | Modified : CoherenceState
  | Invalid : CoherenceState
deriving Repr
