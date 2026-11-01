/**
 * Deterministic local HTTP compatibility fixture. NOT an AI model, not a
 * vendor emulator, not internet-connected. No request body/key logging.
 * Only the fixed routes below respond. Bind strictly to IPv4 loopback.
 */
import { createServer } from 'node:http';

const fixtures = new Map([
  ['/openai/v1/chat/completions', { family: 'openai-chat', check: body => Array.isArray(body.messages), response: { choices: [{ message: { content: '[MOCK openai-chat] Fixture only.' }, finish_reason: 'stop' }] } }],
  ['/responses/v1/responses', { family: 'responses', check: body => Array.isArray(body.input), response: { status: 'completed', output: [{ type: 'message', status: 'completed', content: [{ type: 'output_text', text: '[MOCK responses] Fixture only.' }] }] } }],
  ['/anthropic/v1/messages', { family: 'anthropic', check: body => Array.isArray(body.messages), response: { content: [{ type: 'text', text: '[MOCK anthropic] Fixture only.' }], stop_reason: 'end_turn' } }],
  ['/gemini/v1beta/models/fixture-model:generateContent', { family: 'gemini', check: body => Array.isArray(body.contents), response: { candidates: [{ finishReason: 'STOP', content: { parts: [{ text: '[MOCK gemini] Fixture only.' }] } }] } }],
  ['/ollama/api/chat', { family: 'ollama', check: body => Array.isArray(body.messages), response: { done: true, message: { role: 'assistant', content: '[MOCK ollama] Fixture only.' } } }],
]);

const server = createServer((request, response) => {
  const fixture = fixtures.get(request.url ?? '');
  if (request.method !== 'POST' || !fixture) {
    response.writeHead(404).end();
    return;
  }
  if (fixture.family === 'anthropic' && !request.headers['x-api-key']) {
    response.writeHead(401).end();
    return;
  }
  if (fixture.family === 'gemini' && !request.headers['x-goog-api-key']) {
    response.writeHead(401).end();
    return;
  }
  let size = 0;
  let content = '';
  request.on('data', chunk => {
    size += chunk.length;
    if (size > 80_000) request.destroy();
    else content += chunk.toString('utf8');
  });
  request.on('end', () => {
    try {
      const body = JSON.parse(content);
      if (!fixture.check(body)) {
        response.writeHead(422).end();
        return;
      }
      const json = JSON.stringify(fixture.response);
      response.writeHead(200, { 'content-type': 'application/json', 'content-length': Buffer.byteLength(json), 'cache-control': 'no-store' });
      response.end(json);
    } catch {
      response.writeHead(400).end();
    }
  });
});

server.listen(18769, '127.0.0.1', () => {
  process.stdout.write('Six-protocol local deterministic fixture listening on 127.0.0.1:18769\n');
});
