structure PerCoreMetadata :=
  read_ts : Nat
  write_ts : Nat
  writable : Bool
  modified : Bool

abbrev CoreId := Nat

def State := Array PerCoreMetadata -- CoreId -> CoherenceState


inductive RequestType :=
  | Read : RequestType
  | Write : RequestType
  | Drop : RequestType
deriving Repr

structure Request :=
  coreId : CoreId
  ts : Nat
  ty : RequestType
deriving Repr

abbrev Trace := List Request

def State.handleRequest (s : State) (r : Request) : State :=
  s
