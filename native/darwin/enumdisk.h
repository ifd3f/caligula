#pragma once

#include <inttypes.h>

const uint8_t DEV_TYPE_FILE = 0;
const uint8_t DEV_TYPE_DISK = 1;
const uint8_t DEV_TYPE_PARTITION = 2;

/** A single disk. */
typedef struct Disk {
    /**
        A boolean value representing whether or not we know how big the disk is.
        If this value is 0, we don't know what size it is. Otherwise, we do.
    */
    uint8_t size_is_known;

    /**
        A boolean value representing whether or not we know how big the disk is.
        If this value is 0, we don't know what size it is. Otherwise, we do.
    */
    uint8_t block_size_is_known;

    /**
        A boolean value representing whether or not this disk is removable.
        If this value is 0, it is not removable. If it is 1, then it is removable.
        Any other value means that we don't know if it's removable or not.
    */
    uint8_t is_removable;

    /**
        The type of this device.
    */
    uint8_t dev_type;

    /**
        The file path to write to. This is never null.
        This value is malloced and the user should free it.
    */
    char* bsdname;

    /**
        The model of the disk. This may be null.
        This value is malloced and the user should free it.
    */
    char* model;

    /**
        The size of the disk in bytes, if known.
    */
    uint64_t size;

    /**
        The block size of the disk in bytes, if known.
    */
    uint64_t block_size;
} Disk;

/** A list of disks. */
typedef struct DiskList {
    uint64_t n;
    /** This value is malloced and the user should free it. */
    Disk *disks;
} DiskList;

extern DiskList enumerate_disks();
