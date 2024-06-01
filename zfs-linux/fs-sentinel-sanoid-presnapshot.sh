#!/bin/bash

# before snapshotting, we want to make sure that the given dataset has been
# modified! otherwise, the created snapshot would be redundant. we accomplish
# this by running `fs-sentinel check`, which will conveniently return an exit
# code of 1 if the dataset hasn't been modified!

# NOTE: as of sanoid v2.0.3, this will only be a single dataset;
# this will change in future versions
fs-sentinel check "$SANOID_TARGETS"
