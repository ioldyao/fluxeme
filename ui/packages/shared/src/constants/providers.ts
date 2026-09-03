export const PROVIDER_DISPLAY: Record<string, string> = {
  openai: 'OpenAI',
  aiionly: 'AiOnly',
  anthropic: 'Anthropic',
  vllm: 'vLLM',
  sglang: 'SGLang',
  azure: 'Azure',
  ollama: 'Ollama',
  deepseek: 'DeepSeek',
  dashscope: 'DashScope',
  zhipu: 'Zhipu',
  minimax: 'MiniMax',
  qianfan: '千帆大模型',
  qianfan_token_plan: '千帆大模型 Token Plan',
  volces_ark: '火山方舟',
  volces_agent_plan: '火山方舟 Agent Plan',
  volces_coding_plan: '火山方舟 Coding Plan',
};

export const PROVIDERS = Object.keys(PROVIDER_DISPLAY);
