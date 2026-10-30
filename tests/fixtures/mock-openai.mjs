// LOCAL TEST FIXTURE ONLY. This is not an AI model and must never be presented
// as one. Uses loopback and a fixed port; no credentials or chat text logged.
import { createServer } from 'node:http';

const server = createServer((request, response) => {
  if (request.method !== 'POST' || request.url !== '/v1/chat/completions') {
    response.writeHead(404).end();
    return;
  }
  let body = '';
  request.on('data', (part) => {
    body += part;
    if (body.length > 1_048_576) request.destroy();
  });
  request.on('end', () => {
    try {
      const requestBody = JSON.parse(body);
      if (requestBody.model !== 'fixture-model' || !Array.isArray(requestBody.messages)) {
        response.writeHead(422, { 'Content-Type': 'application/json' }).end('{}');
        return;
      }
      response.writeHead(200, { 'Content-Type': 'application/json' });
      response.end(JSON.stringify({
        id: 'mock-provider-contract',
        object: 'chat.completion',
        choices: [{ index: 0, message: { role: 'assistant', content: '[FIXTURE ONLY] The local test server received your request.' }, finish_reason: 'stop' }],
      }));
    } catch {
      response.writeHead(400).end('{}');
    }
  });
});

server.listen(18767, '127.0.0.1', () => {
  process.stdout.write('Fixture listening on 127.0.0.1:18767\n');
});
