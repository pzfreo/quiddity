#!/bin/bash
# cold import: fresh process, median of 7, after one warm-up
for v in v0312 v051; do
  P=/root/qbench/venvs/$v/bin/python
  $P -W ignore -c "import quiddity" 2>/dev/null
  for i in 1 2 3 4 5 6 7; do /usr/bin/time -f "$v %e s %M KB" $P -W ignore -c "import quiddity" 2>&1 | tail -n 1; done
done
