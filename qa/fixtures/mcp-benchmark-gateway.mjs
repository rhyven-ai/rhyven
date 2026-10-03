// Test-only gateway for separately installed, pinned reference MCP servers.
// It binds loopback, forwards tool definitions/results unchanged, and has no installer.
import http from 'node:http';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
import {randomUUID} from 'node:crypto';
const [modules, fixture] = process.argv.slice(2);
if (!modules || !fixture) throw new Error('Usage: node gateway.mjs NODE_MODULES FIXTURE_DIRECTORY');
const sdk = (file) => import(pathToFileURL(path.resolve(modules, '@modelcontextprotocol/sdk/dist/esm', file)));
const {Client} = await sdk('client/index.js');
const {StdioClientTransport} = await sdk('client/stdio.js');
const commands = {
  filesystem: ['@modelcontextprotocol/server-filesystem/dist/index.js', fixture],
  memory: ['@modelcontextprotocol/server-memory/dist/index.js'],
  everything: ['@modelcontextprotocol/server-everything/dist/index.js', 'stdio'],
};
const sessions = new Map();
const server = http.createServer(async (req, res) => {
  const name = req.url.slice(1);
  const reply = (status, body, headers={}) => {
    const bytes = body === undefined ? '' : JSON.stringify(body);
    res.writeHead(status, {'Content-Type':'application/json','Content-Length':Buffer.byteLength(bytes),...headers});res.end(bytes);
  };
  if (!Object.hasOwn(commands,name)) { reply(404);return; }
  const session = sessions.get(req.headers['mcp-session-id']);
  if (req.method === 'DELETE') {
    if (!session || session.name !== name) {reply(404);return;}
    sessions.delete(req.headers['mcp-session-id']);await session.client.close();reply(204);return;
  }
  if (req.method !== 'POST') {reply(405);return;}
  let bytes = ''; let message;
  try {
    for await (const chunk of req) {bytes+=chunk;if(Buffer.byteLength(bytes)>1048576)throw new Error('Request too large');}
    message = JSON.parse(bytes);
    if (message.method === 'initialize') {
      const client = new Client({name:'local-benchmark-gateway',version:'1'},{capabilities:{}});
      const [script,...args]=commands[name];
      const transport = new StdioClientTransport({command:process.execPath,args:[path.resolve(modules,script),...args],
        env:{PATH:process.env.PATH || '',MEMORY_FILE_PATH:path.resolve(fixture,'memory.json')},stderr:'pipe'});
      try {await client.connect(transport);} catch(e) {await client.close();throw e;}
      // Keep logs out of the protocol and drain to avoid blocked child pipes.
      transport.stderr?.resume();
      const id = randomUUID();sessions.set(id,{name,client});
      reply(200,{jsonrpc:'2.0',id:message.id,result:{protocolVersion:'2025-11-25',capabilities:client.getServerCapabilities(),
        serverInfo:client.getServerVersion(),instructions:client.getInstructions() || ''}}, {'Mcp-Session-Id':id});return;
    }
    if (!session || session.name !== name) {reply(404);return;}
    if (message.method === 'notifications/initialized') {reply(202);return;}
    let result;
    if (message.method === 'tools/list') result=await session.client.listTools(message.params);
    else if (message.method === 'tools/call') result=await session.client.callTool(message.params);
    else throw new Error('Only tool discovery/calls supported by test gateway');
    reply(200,{jsonrpc:'2.0',id:message.id,result});
  } catch(error) {reply(200,{jsonrpc:'2.0',id:message?.id ?? null,error:{code:-32603,message:String(error.message)}});}
});
server.listen(0,'127.0.0.1',()=>process.stdout.write(JSON.stringify({endpoint:`http://127.0.0.1:${server.address().port}`})+'\n'));
async function close() {await Promise.allSettled([...sessions.values()].map(s=>s.client.close()));server.close(()=>process.exit(0));}
process.on('SIGTERM',close);process.on('SIGINT',close);
