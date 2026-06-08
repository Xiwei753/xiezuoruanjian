1. **Analyze the testing gap:**
   - The `rename_project` function is currently missing tests for renaming a project.
   - Specifically, we need to test what happens when a project is renamed to a title that already exists in another project.
   - Because projects use UUIDs as their directory names, this should succeed without file collisions.

2. **Design the test strategy:**
   - Add a test `test_rename_project` to ensure renaming works in the normal case.
   - Add a test `test_rename_project_duplicate_title` to verify that renaming a project to an already existing title succeeds and doesn't cause any file collision or error.
   - Add a test `test_rename_project_not_found` to ensure renaming a non-existent project returns `Error::ProjectNotFound`.

3. **Implement the tests:**
   - Modify `core/writer_core/src/project_tests.rs` to include these tests.
   - Ensure the new tests pass and do not affect existing functionality.

4. **Verify the tests:**
   - Run the test suite: `cargo test -p writer_core -- test_rename_project`.
   - Complete pre-commit instructions.
   - Submit the PR.
