export const PROVIDER_DISPLAY: Record<string, string> = {
  openai: 'OpenAI',
  anthropic: 'Anthropic',
  vllm: 'vLLM',
  sglang: 'SGLang',
  azure: 'Azure',
  ollama: 'Ollama',
  deepseek: 'DeepSeek',
  dashscope: 'DashScope',
  zhipu: 'Zhipu',
  minimax: 'MiniMax',
};

export const PROVIDERS = Object.keys(PROVIDER_DISPLAY);
