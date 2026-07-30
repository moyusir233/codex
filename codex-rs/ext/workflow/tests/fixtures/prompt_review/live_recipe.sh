#!/bin/sh
set -eu

: "${PROMPT_REVIEW_LIVE_CONFIRMED:?set to yes after approving this live test}"
: "${PROMPT_REVIEW_PROMPT_KEY:?set a disposable Fornax prompt key}"
: "${PROMPT_REVIEW_LARK_USERS:?set consenting comma-delimited ou_ IDs}"
: "${PROMPT_REVIEW_LARK_CHAT_ID:?set the disposable private oc_ chat ID}"

if [ "$PROMPT_REVIEW_LIVE_CONFIRMED" != "yes" ]; then
  echo "PROMPT_REVIEW_LIVE_CONFIRMED must equal yes" >&2
  exit 2
fi

exec codex workflow prompt-review \
  --prompt-key "$PROMPT_REVIEW_PROMPT_KEY" \
  --reviewers 3 \
  --lark-users "$PROMPT_REVIEW_LARK_USERS" \
  --lark-chat-id "$PROMPT_REVIEW_LARK_CHAT_ID"
