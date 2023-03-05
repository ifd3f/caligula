/** A single disk. */
typedef struct Disk {
    /**
        The file path to write to. This is never null.
        This value is malloced and the user should free it.
    */
    char* devnode;

    /**
        The model of the disk. This may be null.
        This value is malloced and the user should free it.
    */
    char* model;

    /**
        A boolean value representing whether or not we know how big the disk is.
        If this value is 0, we don't know what size it is. Otherwise, we do.
    */
    int size_is_known;

    /**
        A boolean value representing whether or not this disk is removable.
        If this value is 0, it is not removable. If it is 1, then it is removable.
        Any other value means that we don't know if it's removable or not.
    */
    int is_removable;

    /**
        The size of the disk in bytes, if known.
    */
    unsigned long size;
} Disk;

/** A list of disks. */
typedef struct DiskList {
    unsigned long n;
    /** This value is malloced and the user should free it. */
    Disk *disks;
} DiskList;

DiskList enumerate_disks();
