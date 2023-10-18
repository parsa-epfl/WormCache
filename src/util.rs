use std::collections::HashMap;

// Return value: Map[first_index, size]
pub fn find_fetch_block_from_block_id_sequence(i: Vec<usize>) -> HashMap<usize, usize> {
    let mut res = HashMap::new();
    let mut last_fb_block_id: Option<usize> = None;
    let mut last_fb_first_instruction_index: usize = 0;
    let total = i.len();

    // traverse the array and find the inconsistent position.
    for (idx, block_id) in i.into_iter().enumerate() {
        match last_fb_block_id {
            Some(last_fb_id) => {
                if block_id != last_fb_id {
                    // We start a new block.
                    // First, keep the old block.
                    let last_fb_size = idx - last_fb_first_instruction_index;
                    res.insert(last_fb_first_instruction_index, last_fb_size);
                    // Then, adjust the information of the last block
                    last_fb_first_instruction_index = idx;
                    last_fb_block_id = Some(block_id);
                }
            }
            None => {
                last_fb_block_id = Some(block_id);
            }
        }
    }

    // insert the last element as well.
    res.insert(
        last_fb_first_instruction_index,
        total - last_fb_first_instruction_index,
    );

    return res;
}

#[test]
fn test_find_fetch_block_from_pa_sequence() {
    let example = vec![0, 0, 1, 1, 2, 2, 3, 3, 3];
    let res = find_fetch_block_from_block_id_sequence(example);
    assert_eq!(res.len(), 4);
    assert_eq!(res.get(&0), Some(&2));
    assert_eq!(res.get(&2), Some(&2));
    assert_eq!(res.get(&4), Some(&2));
    assert_eq!(res.get(&6), Some(&3));

    let example = vec![0];
    let res = find_fetch_block_from_block_id_sequence(example);
    assert_eq!(res.len(), 1);
    assert_eq!(res.get(&0), Some(&1));

    let example = vec![1];
    let res = find_fetch_block_from_block_id_sequence(example);
    assert_eq!(res.len(), 1);
    assert_eq!(res.get(&0), Some(&1));

    let example = vec![1,2,2,10,10];
    let res = find_fetch_block_from_block_id_sequence(example);
    assert_eq!(res.len(), 3);
    assert_eq!(res.get(&0), Some(&1));
    assert_eq!(res.get(&1), Some(&2));
    assert_eq!(res.get(&3), Some(&2));
}