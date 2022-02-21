import { analyze } from './index.js';

async function main() {
  for await (const [file, dep] of analyze(['/home/abi/tvlk/www/packages/core/index.ts'])) {
    console.log(file, dep)
  }
}

main()
