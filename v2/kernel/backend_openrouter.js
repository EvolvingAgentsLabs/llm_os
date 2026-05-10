// kernel/backend_openrouter.js
// OpenRouter backend for the LLM-OS v2 kernel.
// Talks to any model via OpenRouter's chat completions API.
// Supports structured output via JSON schema and GBNF grammar
// passthrough (for models/providers that support it).

const DEFAULT_BASE_URL = 'https://openrouter.ai/api/v1';

export class OpenRouterBackend {
  /**
   * @param {object} opts
   * @param {string} opts.apiKey        - OpenRouter API key (or OPENROUTER_API_KEY env)
   * @param {string} [opts.model]       - Model ID (e.g. 'google/gemma-4-31b-it')
   * @param {string} [opts.baseUrl]     - API base URL
   * @param {number} [opts.maxTokens]   - Max tokens per generation
   * @param {number} [opts.temperature] - Sampling temperature
   */
  constructor(opts = {}) {
    this.apiKey = opts.apiKey || process.env.OPENROUTER_API_KEY || '';
    this.model = opts.model || 'google/gemma-4-31b-it';
    this.baseUrl = opts.baseUrl || DEFAULT_BASE_URL;
    this.maxTokens = opts.maxTokens || 512;
    this.temperature = opts.temperature ?? 0.3;
    if (!this.apiKey) {
      throw new Error('OpenRouter API key required. Set OPENROUTER_API_KEY or pass opts.apiKey');
    }
  }

  /**
   * Generate a completion from messages.
   *
   * @param {Array<{role: string, content: string}>} messages - Chat messages
   * @param {object} [opts]
   * @param {object} [opts.responseFormat] - JSON schema for structured output
   * @param {string[]} [opts.stop]         - Stop sequences
   * @param {number} [opts.maxTokens]      - Override max tokens
   * @param {number} [opts.temperature]    - Override temperature
   * @returns {Promise<{content: string, usage: object, model: string, finishReason: string}>}
   */
  async generate(messages, opts = {}) {
    const body = {
      model: this.model,
      messages,
      max_tokens: opts.maxTokens || this.maxTokens,
      temperature: opts.temperature ?? this.temperature,
    };

    if (opts.stop) {
      body.stop = opts.stop;
    }

    if (opts.responseFormat) {
      body.response_format = opts.responseFormat;
    }

    const res = await fetch(`${this.baseUrl}/chat/completions`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${this.apiKey}`,
        'HTTP-Referer': 'https://github.com/EvolvingAgentsLabs/llm_os',
        'X-Title': 'LLM-OS v2',
      },
      body: JSON.stringify(body),
    });

    if (!res.ok) {
      const text = await res.text();
      throw new Error(`OpenRouter ${res.status}: ${text}`);
    }

    const json = await res.json();
    const choice = json.choices?.[0];

    if (!choice) {
      throw new Error(`OpenRouter returned no choices: ${JSON.stringify(json)}`);
    }

    return {
      content: choice.message?.content || '',
      finishReason: choice.finish_reason || 'unknown',
      model: json.model || this.model,
      usage: json.usage || {},
    };
  }
}
