// const axios = require('axios');

const runTest = async () => {

  const resp1 = await fetch('http://127.0.0.1:8000/long');
  console.log(resp1.status)
  if (resp1.status !== 500) {
    throw 'Unexpected status!'
  }
  const resp2 = await fetch('http://127.0.0.1:8000/default');
  if (resp2.status !== 200) {
    throw 'Unexpected status!'
  }
  console.log(resp2.status)
};


function sleep(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}


const runTestParallel1 = async () => {
  const arr = [];
  
  for (let i = 0; i < 100; i++) {
    arr.push(0);
  }
  console.log('Sending tons of requests...');
  for (let k = 0; k < 100; k++) {
    const manyFetches = arr.map(() =>
      fetch('http://127.0.0.1:8000/default').then(res => res.status));
    const res = await Promise.all(manyFetches);
    const hasError = res.find(a => a !== 200);
    if (hasError) {
      console.log({res})
      throw 'Unexpected error status!';
    }
  }
  await sleep(1000);
  const res = await fetch('http://127.0.0.1:8000/default');
  console.log(res.status);
  if (res.status !== 200) {
    throw 'Unexpected error status!';
  }
};

runTest();
runTestParallel1();
