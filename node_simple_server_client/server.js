const http = require('http');

function sleep(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}

const requestListener = async function (req, res) {
  await sleep(2000);  
  res.writeHead(200);
  const d = new Date();
  res.end('Hello, World! Current Time: ' + d.toLocaleString());
}

const server = http.createServer(requestListener);
console.log('listening on 8080');
server.listen(8080);

