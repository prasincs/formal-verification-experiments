use verus_builtin_macros::verus;

#[cfg(verus_keep_ghost)]
use vstd::prelude::*;

pub const LEGACY_GENERATION: u32 = 0;
pub const FIRST_STABLE_GENERATION: u32 = 2;

verus! {

pub fn validate_stable_generation(observed: u32) -> (result: Result<u32, u32>)
    ensures
        result.is_ok() <==> observed % 2u32 == 0u32,
        result.is_ok() ==> result.unwrap() == observed,
        result.is_err() ==> result.unwrap_err() == observed,
{
    if observed % 2u32 == 0u32 {
        Ok(observed)
    } else {
        Err(observed)
    }
}

pub fn reset_plan(current: u32) -> (result: Result<(u32, u32), u32>)
    ensures
        result.is_err() <==> current % 2u32 == 1u32,
        result.is_ok() ==> result.unwrap().0 % 2u32 == 1u32,
        result.is_ok() ==> result.unwrap().1 % 2u32 == 0u32,
        result.is_ok() ==> result.unwrap().1 != LEGACY_GENERATION,
        result.is_ok() && current < 0xffff_fffeu32 ==>
            result.unwrap().1 as int == current as int + 2,
        result.is_ok() && current >= 0xffff_fffeu32 ==>
            result.unwrap().1 == FIRST_STABLE_GENERATION,
{
    if current % 2u32 == 1u32 {
        Err(current)
    } else {
        let odd = current + 1u32;
        let next_even = if current >= 0xffff_fffeu32 {
            FIRST_STABLE_GENERATION
        } else {
            current + 2u32
        };
        Ok((odd, next_even))
    }
}

} // verus!
