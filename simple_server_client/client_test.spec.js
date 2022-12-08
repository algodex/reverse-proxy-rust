// const axios = require('axios');

const runTest = async () => {

  const resp1 = await fetch('http://127.0.0.1:8000/long');
  console.log({resp1})
  const resp2 = await fetch('http://127.0.0.1:8000/default');
  console.log({resp2})
};


function sleep(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}


const runTestParallel1 = async () => {
  const arr = [];
  
  for (let i = 0; i < 100; i++) {
    arr.push(0);
  }
  while (1) {
    const manyFetches = arr.map(() =>
      fetch('http://127.0.0.1:8000/default').then(res => res.status));
    const res = await Promise.all(manyFetches);
    const hasError = res.find(a => a !== 200);
    if (hasError) {
      console.log({res})
      break;
    }
  }
  await sleep(1000);
  const res = await fetch('http://127.0.0.1:8000/default');
  console.log(res.status);
};

runTestParallel1();
