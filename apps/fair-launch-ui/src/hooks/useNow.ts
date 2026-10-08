import { useEffect, useState } from 'react';

const nowSeconds = (): bigint => BigInt(Math.floor(Date.now() / 1000));

/** The wall-clock time in unix seconds, refreshed every second (the chain's clock stays within seconds of it). */
export function useNow(): bigint {
  const [now, setNow] = useState(nowSeconds);
  useEffect(() => {
    const timer = setInterval(() => setNow(nowSeconds()), 1000);
    return () => clearInterval(timer);
  }, []);
  return now;
}
