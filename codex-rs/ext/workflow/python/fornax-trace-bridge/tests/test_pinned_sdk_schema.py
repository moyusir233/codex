import json

from bytedance.fornax.infra.trace.model import Message, ModelInput


def test_pinned_sdk_model_input_uses_messages_not_documented_typo() -> None:
    encoded = json.loads(ModelInput(messages=[Message(role="user", content="safe")]).to_json())
    assert encoded == {"messages": [{"role": "user", "content": "safe"}]}
    assert "messsages" not in encoded
