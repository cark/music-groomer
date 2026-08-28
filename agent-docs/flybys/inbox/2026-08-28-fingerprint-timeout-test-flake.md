# Fingerprint timeout test flaked once

The full locked suite once saw
`terminates_a_helper_that_exceeds_the_deadline` return something other than
`FingerprintError::Timeout`. The exact test and an immediate full-suite rerun
passed. Triage the deadline fixture for scheduler sensitivity at the next
milestone boundary; do not broaden the PNG-probe correction into this work.
