const http = require('http');

function sleep(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}

const requestListener = async function (req, res) {
  await sleep(8000);  
  res.writeHead(200);
  const d = new Date();
  res.end('Hello, World! Current Time: ' + d.toLocaleString());
}

const server = http.createServer(requestListener);
const port = 6000;
console.log('listening on ' + port);
server.listen(port);

