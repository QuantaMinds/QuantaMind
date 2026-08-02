/// A chat template for an the server `Modelfile`. `template_string` is the
/// raw Go-template body (uses {{ .System }} / {{ .Prompt }} / {{ .Response }}),
/// `stop_tokens` are the strings that terminate a generation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChatTemplate {
    pub family: &'static str,
    pub template_string: &'static str,
    pub stop_tokens: &'static [&'static str],
}

pub const LLAMA3: ChatTemplate = ChatTemplate {
    family: "Llama 3",
    template_string: "<|begin_of_text|><|start_header_id|>system<|end_header_id|>\n\n{{ .System }}<|eot_id|><|start_header_id|>user<|end_header_id|>\n\n{{ .Prompt }}<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n\n{{ .Response }}<|eot_id|>",
    stop_tokens: &["<|eot_id|>", "<|end_of_text|>"],
};

pub const QWEN_CHATML: ChatTemplate = ChatTemplate {
    family: "Qwen",
    template_string: "{{ if .System }}<|im_start|>system\n{{ .System }}<|im_end|>\n{{ end }}<|im_start|>user\n{{ .Prompt }}<|im_end|>\n<|im_start|>assistant\n{{ .Response }}<|im_end|>",
    stop_tokens: &["<|im_end|>", "<|endoftext|>"],
};

pub const MISTRAL: ChatTemplate = ChatTemplate {
    family: "Mistral",
    template_string: "[INST] {{ if .System }}{{ .System }}\n\n{{ end }}{{ .Prompt }} [/INST] {{ .Response }}</s>",
    stop_tokens: &["</s>"],
};

pub const PHI3: ChatTemplate = ChatTemplate {
    family: "Phi-3",
    template_string: "{{ if .System }}<|system|>\n{{ .System }}<|end|>\n{{ end }}<|user|>\n{{ .Prompt }}<|end|>\n<|assistant|>\n{{ .Response }}<|end|>",
    stop_tokens: &["<|end|>", "<|endoftext|>"],
};

pub const GEMMA: ChatTemplate = ChatTemplate {
    family: "Gemma",
    template_string: "<start_of_turn>user\n{{ if .System }}{{ .System }}\n\n{{ end }}{{ .Prompt }}<end_of_turn>\n<start_of_turn>model\n{{ .Response }}<end_of_turn>",
    stop_tokens: &["<end_of_turn>"],
};

/// OpenAI gpt-oss / "harmony" format. The end-of-generation tokens are `<|return|>`
/// (final answer done) and `<|call|>` (a tool call is ready to execute) — NOT a plain
/// EOS. WITHOUT these as stops the model emits them as literal text and never halts,
/// hallucinating its own multi-turn transcript (the infinite-generation bug).
/// `<|end|>` is DELIBERATELY excluded: it only ends an INTERMEDIATE message within a
/// turn, so stopping on it truncates the turn before the model can emit its tool call.
/// `template_string` is the create-path Modelfile body; full harmony channel/reasoning
/// rendering is a separate follow-up — the loop fix relies on `stop_tokens`, not on this.
pub const GPT_OSS: ChatTemplate = ChatTemplate {
    family: "GPT-OSS (harmony)",
    template_string: "<|start|>system<|message|>{{ .System }}<|end|><|start|>user<|message|>{{ .Prompt }}<|end|><|start|>assistant",
    stop_tokens: &["<|return|>", "<|call|>"],
};

pub const COMMAND_R: ChatTemplate = ChatTemplate {
    family: "Command-R",
    template_string: "<|START_OF_TURN_TOKEN|><|SYSTEM_TOKEN|>{{ .System }}<|END_OF_TURN_TOKEN|><|START_OF_TURN_TOKEN|><|USER_TOKEN|>{{ .Prompt }}<|END_OF_TURN_TOKEN|><|START_OF_TURN_TOKEN|><|CHATBOT_TOKEN|>{{ .Response }}<|END_OF_TURN_TOKEN|>",
    stop_tokens: &["<|END_OF_TURN_TOKEN|>"],
};

pub const DEEPSEEK: ChatTemplate = ChatTemplate {
    family: "DeepSeek",
    template_string: "{{ if .System }}{{ .System }}\n\n{{ end }}### Instruction:\n{{ .Prompt }}\n\n### Response:\n{{ .Response }}\n<|EOT|>",
    stop_tokens: &["<|EOT|>"],
};

pub const YI: ChatTemplate = ChatTemplate {
    family: "Yi",
    template_string: "{{ if .System }}<|im_start|>system\n{{ .System }}<|im_end|>\n{{ end }}<|im_start|>user\n{{ .Prompt }}<|im_end|>\n<|im_start|>assistant\n{{ .Response }}<|im_end|>",
    stop_tokens: &["<|im_end|>", "<|endoftext|>"],
};
