const http = require('http');

function sleep(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}

const longRequest = async function (req, res) {
  await sleep(8000);
  res.writeHead(200);
  const d = new Date();
  // console.log({req});
  res.end('Long Request! Current Time: ' + d.toLocaleString());
}


const fastRequest = async function (req, res) {
  await sleep(700);
  res.writeHead(200);
  const d = new Date();
  // console.log({req});
  res.end('Fast Request! Current Time: ' + d.toLocaleString());
}


const defaultRequest = async function (req, res) {
  await sleep(4000);
  res.writeHead(200);
  const d = new Date();
  // console.log({req});
  res.end('Default request! Current Time: ' + d.toLocaleString());
}


const requestListener = async function (req, res) {
  console.log('in req listener: '+ req.url);
  if (req.url === '/long') {
    return await longRequest(req, res);
  }
  if (req.url === '/fast') {
    return await fastRequest(req, res);
  }
  return defaultRequest(req, res);
}

const server = http.createServer(requestListener);
const port = 6000;
console.log('listening on ' + port);
server.listen(port);

