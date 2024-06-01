#!/bin/bash

# after snapshotting, we want to reset the status of the snapshotted dataset to "unmodified"!

# NOTE: as of sanoid v2.0.3, this will only be a single dataset;
# this will change in future versions
fs-sentinel mark "$SANOID_TARGETS"
