data MemoryAction = GetCopy | GetExclusive | InvalidLine;
data CoherenceState = Modified | Owned | Shared | Exclusive | Invalid deriving Show;
data CopyInfo = NoCopy | OneCleanCopy | OneDirtyCopy | MultipleCleanCopy | MultipleCleanCopyAndOneDirtyCopy deriving Show;
data CoherenceResults = Results {
    new_private_state :: CoherenceState,
    new_other_state :: CopyInfo
} deriving Show;

-- | The transition function of a coherence state
-- Parameters:
--  private_state
--  memory_action
--  copy_info
-- Returns:
--  new private state
--  new copy info
-- 
-- This function is not finished yet.
coherenceTransition :: CoherenceState -> MemoryAction -> CopyInfo -> CoherenceResults
coherenceTransition co m cpy = Results {
    new_private_state = Invalid,
    new_other_state = NoCopy
}


