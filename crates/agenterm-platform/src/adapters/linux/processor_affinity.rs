use crate::contract::processor_affinity::{
    LogicalProcessorLocation, ProcessorAffinityError, ProcessorAffinityErrorKind,
    ProcessorAffinityFacts, ProcessorSetSemantics,
};

const INITIAL_WORDS: usize = 16;
const MAX_WORDS: usize = 16 * 1024;

pub(crate) fn current_process() -> Result<ProcessorAffinityFacts, ProcessorAffinityError> {
    let words = query_words()?;
    let processors = words
        .iter()
        .enumerate()
        .flat_map(|(word_index, word)| {
            (0..usize::BITS).filter_map(move |bit| {
                if word & (1_usize << bit) == 0 {
                    return None;
                }
                let index = word_index
                    .checked_mul(usize::BITS as usize)?
                    .checked_add(bit as usize)?;
                Some(LogicalProcessorLocation {
                    group: 0,
                    index: u32::try_from(index).ok()?,
                })
            })
        })
        .collect();
    ProcessorAffinityFacts::from_locations(processors, ProcessorSetSemantics::SchedulerAllowed)
}

fn query_words() -> Result<Vec<usize>, ProcessorAffinityError> {
    let mut word_count = INITIAL_WORDS;
    loop {
        let mut words = vec![0_usize; word_count];
        let byte_count = words
            .len()
            .checked_mul(std::mem::size_of::<usize>())
            .ok_or_else(|| {
                ProcessorAffinityError::new(
                    ProcessorAffinityErrorKind::InvalidValue,
                    "affinity buffer size overflow",
                )
            })?;
        let result = unsafe {
            libc::sched_getaffinity(0, byte_count, words.as_mut_ptr().cast::<libc::cpu_set_t>())
        };
        if result == 0 {
            return Ok(words);
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EINVAL) || word_count >= MAX_WORDS {
            return Err(ProcessorAffinityError::new(
                ProcessorAffinityErrorKind::Query,
                format!("sched_getaffinity: {error}"),
            ));
        }
        word_count = word_count.checked_mul(2).ok_or_else(|| {
            ProcessorAffinityError::new(
                ProcessorAffinityErrorKind::InvalidValue,
                "affinity buffer growth overflow",
            )
        })?;
    }
}
