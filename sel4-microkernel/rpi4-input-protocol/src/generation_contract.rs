use verus_builtin_macros::verus;

use crate::{FIRST_STABLE_GENERATION, LEGACY_GENERATION};

verus! {

/// Executable generation validation used by the runtime `resync` path.
pub fn validate_stable_generation(observed: u32) -> (result: Result<u32, u32>)
    ensures
        result.is_ok() <==> observed % 2 == 0,
        result.is_ok() ==> result.unwrap() == observed,
        result.is_err() ==> result.unwrap_err() == observed,
{
    if observed % 2 == 0 {
        Ok(observed)
    } else {
        Err(observed)
    }
}

/// Executable arithmetic used by the atomic reset implementation.
///
/// The caller supplies the linear `EndpointsStopped` token; this function
/// proves the resulting publication plan is odd followed by stable nonzero
/// even, including wraparound away from legacy generation zero.
pub fn reset_plan(current: u32) -> (result: Result<(u32, u32), u32>)
    ensures
        result.is_err() <==> current % 2 == 1,
        result.is_ok() ==> result.unwrap().0 % 2 == 1,
        result.is_ok() ==> result.unwrap().1 % 2 == 0,
        result.is_ok() ==> result.unwrap().1 != LEGACY_GENERATION,
        result.is_ok() ==> result.unwrap().1 ==
            if current >= 0xffff_fffe { FIRST_STABLE_GENERATION } else { current + 2 },
{
    if current % 2 == 1 {
        Err(current)
    } else {
        let odd = current + 1;
        let next_even = if current >= 0xffff_fffe {
            FIRST_STABLE_GENERATION
        } else {
            current + 2
        };
        Ok((odd, next_even))
    }
}

} // verus!
