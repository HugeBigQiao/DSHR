// 假的 dsh --profile sdk runtime：stdio JSON-RPC 最小实现，供集成测试用。
// 对应官方先例：packages/sdk/client/tests/fake-runtime.ts。
// 行为：initialize → 应答；session/prompt → 应答 messageId + 发回执/assistant/message/idle；
//       shutdown → 应答并退出；未知方法 → JSON-RPC error。
import readline from 'node:readline'

const rl = readline.createInterface({ input: process.stdin })
const send = (obj) => process.stdout.write(JSON.stringify(obj) + '\n')

rl.on('line', (line) => {
  let msg
  try {
    msg = JSON.parse(line)
  } catch {
    return
  }
  if (msg.method === 'initialize') {
    send({ jsonrpc: '2.0', id: msg.id, result: { serverInfo: { name: 'fake-runtime', version: '0.0.0' } } })
  } else if (msg.method === 'session/prompt') {
    const messageId = `msg-${msg.id}`
    const sessionId = msg.params.sessionId
    // 入队回执
    send({ jsonrpc: '2.0', id: msg.id, result: { messageId } })
    // 事件流：inbox 回执 → assistant/message → idle（模拟一次完整 run）
    send({
      jsonrpc: '2.0', method: 'session.event',
      params: {
        sessionId,
        event: {
          type: 'agent/inbox/spliced', seq: 1, time: Date.now(),
          data: {
            target: 'next-turn', start: 0,
            inserted: [{ id: messageId, role: 'user', content: msg.params.contentBlocks, source: { kind: 'user' } }],
          },
        },
      },
    })
    send({
      jsonrpc: '2.0', method: 'session.event',
      params: {
        sessionId,
        event: {
          type: 'assistant/message', seq: 2, time: Date.now(),
          data: {
            turn: 1, step: 1,
            message: {
              id: 'a1', role: 'assistant',
              content: [{ type: 'text', text: 'hello from fake' }],
              source: { kind: 'model', provider: 'fake', model: 'fake-model' },
            },
            usage: { inputTokens: 1, outputTokens: 2 },
          },
        },
      },
    })
    send({ jsonrpc: '2.0', method: 'session.status', params: { sessionId, status: 'idle' } })
  } else if (msg.method === 'shutdown') {
    send({ jsonrpc: '2.0', id: msg.id, result: {} })
    process.exit(0)
  } else {
    send({ jsonrpc: '2.0', id: msg.id, error: { code: -32601, message: 'method not found' } })
  }
})
