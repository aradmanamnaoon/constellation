// Deterministic, local-only cancellation fixture. Not an AI model.
// Never logs request bodies or authorization headers.
import { createServer } from 'node:http';

const server = createServer((request, response) => {
  if (request.method !== 'POST' || request.url !== '/v1/chat/completions') {
    response.writeHead(404).end();
    return;
  }
  let received = 0;
  request.on('data', (chunk) => {
    received += chunk.length;
    if (received > 65_536) request.destroy();
  });
  request.on('end', () => {
    setTimeout(() => {
      if (response.destroyed) return;
      response.writeHead(200, { 'Content-Type': 'application/json' });
      response.end(JSON.stringify({
        id: 'delayed-fixture-only',
        object: 'chat.completion',
        choices: [{
          index: 0,
          message: { role: 'assistant', content: '[DELAYED FIXTURE ONLY] Cancellation regression response.' },
          finish_reason: 'stop'
        }]
      }));
    }, 6_000);
  });
});
server.listen(18768, '127.0.0.1', () => {
  process.stdout.write('Delayed fixture listening on 127.0.0.1:18768\n');
});
