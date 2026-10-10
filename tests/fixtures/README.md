# Signing test fixture

The P12 file and self-signed certificate in this directory are generated solely for regression tests. The P12 password is fixture-only. This identity is public test data and must never be used for real approvals or production signing.

The executable does not embed or use this test identity. Signing requires the user's own P12/PFX certificate chosen in the signing dialog.
