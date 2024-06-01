#!/bin/bash

# monitors all mounted ZFS datasets

IFS=$'\n'

RES=($(zfs list -Ho name,mountpoint | grep -E --invert-match -- '(none|-)$' | sed 's/\t/=/'))

exec fs-sentinel daemon "${RES[@]}"
