# Fixture Manifest

## Current committed smoke set

### `quiet_speech/quiet_intro.wav`

- Text: `Quiet speech benchmark. Dictum should capture low volume phrases clearly.`
- Source: Windows speech synthesis
- Post-process: reduced amplitude

### `whisper_speech/whisper_intro.wav`

- Text: `Whisper speech benchmark. Soft speech should still be recognized.`
- Source: Windows speech synthesis
- Post-process: stronger amplitude reduction

### `noisy_room/noisy_intro.wav`

- Text: `Noisy room benchmark. Background sound should not block the transcript.`
- Source: Windows speech synthesis
- Post-process: reduced amplitude with synthetic noise mix

### `long_form/long_paragraph.wav`

- Text: `Long form benchmark. Dictum should remain stable across a longer paragraph of dictated text with multiple clauses and a natural speaking rhythm for testing latency quality and fallback behavior.`
- Source: Windows speech synthesis
- Post-process: moderate amplitude reduction

## Next fixture upgrade

Replace or supplement this smoke set with human-recorded samples for:

- genuinely quiet speech
- true whisper speech
- real ambient-noise environments
- longer natural dictation passages
