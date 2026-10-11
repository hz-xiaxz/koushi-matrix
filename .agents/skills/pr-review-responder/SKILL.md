---
name: pr-review-responder
description: Handle GitHub pull-request review feedback by inspecting inline threads, applying and verifying code fixes, replying in the original review thread, and resolving the thread after the fix is pushed.
---

# Pull request review responder

Use this skill when the user asks to address, respond to, or resolve a GitHub
pull-request review comment.

## Workflow

1. Identify the repository, pull request, review, and inline comment from the
   supplied URL. Prefer `gh api` over page scraping:

   - `gh api --paginate repos/OWNER/REPO/pulls/NUMBER/reviews`
   - `gh api --paginate repos/OWNER/REPO/pulls/NUMBER/comments?per_page=100`
   - For thread state, query GraphQL `pullRequest.reviewThreads` with
     `pageInfo { hasNextPage endCursor }`, repeat with `after: endCursor`, and
     match the comment's `databaseId`.

2. Read the cited code and the relevant repository contracts before editing.
   If the review identifies stale documentation or a state-machine contract,
   update that contract in the same change as the implementation.

3. Implement the smallest complete fix. Preserve unrelated worktree changes.
   Do not weaken tests, security checks, signing requirements, or release
   gates to silence a review comment.

4. Run proportionate validation, including formatting, type checks, focused
   tests, and `git diff --check` where applicable. If a test exposes a
   follow-up defect, fix it before replying and report the final validation
   result accurately.

5. Commit and push the fix to the pull request branch only after the user has
   authorized the external mutation. Keep the commit focused.

6. Reply to the original inline review comment, not merely to the pull request
   conversation. Use GitHub's review-comment reply endpoint:

   ```powershell
   gh api repos/OWNER/REPO/pulls/NUMBER/comments `
     -f body="..." `
     -F in_reply_to=COMMENT_DATABASE_ID
   ```

   The reply should state what changed, the relevant commit, and validation
   status. Do not use `gh pr comment` as a substitute when the user asked for
   a threaded reply.

7. After the fix is pushed and the threaded reply succeeds, automatically
   resolve the original review thread. Obtain the actual GraphQL thread node
   ID; a
   review-comment node ID (`PRRC_...`) is not a review-thread ID (`PRRT_...`):

   ```powershell
   gh api graphql -f query='mutation($threadId:ID!){resolveReviewThread(input:{threadId:$threadId}){thread{isResolved}}}' `
     -F threadId='THREAD_NODE_ID'
   ```

8. Verify the thread is resolved and report the reply URL, commit, validation,
   and any remaining CI or review status.

## Safety boundaries

- A review URL may identify a review summary, an inline comment, or the PR's
  issue conversation. Inspect the API object before choosing the reply target.
- Never claim a review thread was replied to when only a general PR comment was
  posted.
- Do not merge, rerun workflows, or alter release artifacts unless the user
  explicitly requests that separate action.
- Keep secrets and private account data out of replies, logs, fixtures, and
  test artifacts.
