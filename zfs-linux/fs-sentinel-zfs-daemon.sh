#!/bin/bash

IFS=$'\n'

RES=($(zfs list -Ho name,mountpoint | grep --invert-match -- '-$' | sed 's/\t/=/'))

exec fs-sentinel daemon "${RES[@]}"
