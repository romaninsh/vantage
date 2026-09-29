// Two good steps, then an uncaught exception.
sleep(seconds(1));
sleep(seconds(1));
throw "chaos: failing on the third step";
