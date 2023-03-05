// A lot of it was borrowed from: https://github.com/balena-io-modules/drivelist/blob/master/src/darwin/list.mm

extern "C" {
#import "enumdisk.h"
}

#import <Cocoa/Cocoa.h>
#import <Foundation/Foundation.h>
#import <DiskArbitration/DiskArbitration.h>

NSNumber *DictionaryGetNumber(CFDictionaryRef dict, const void *key) {
  return (NSNumber*)CFDictionaryGetValue(dict, key);
}

char *MakeOwnedCStr(const char *s) {
  char* out = (char*)malloc(strlen(s) + 1);
  strcpy(out, s);
  return out;
}

extern DiskList enumerate_disks() {
  DASessionRef session = DASessionCreate(kCFAllocatorDefault);

  // Add mount points
  NSArray *volumeKeys = [NSArray arrayWithObjects:NSURLVolumeNameKey, NSURLVolumeLocalizedNameKey, nil];
  NSArray *volumePaths = [
    [NSFileManager defaultManager]
    mountedVolumeURLsIncludingResourceValuesForKeys:volumeKeys
    options:0
  ];

  DiskList list;
  list.n = 0;
  list.disks = (Disk*)malloc(0);

  for (NSURL *path in volumePaths) {
    DADiskRef disk = DADiskCreateFromVolumePath(kCFAllocatorDefault, session, (__bridge CFURLRef)path);
    if (disk == nil) {
      continue;
    }

    const char *bsdnameChar = DADiskGetBSDName(disk);
    if (bsdnameChar == nil) {
      CFRelease(disk);
      continue;
    }

    CFDictionaryRef diskinfo = DADiskCopyDescription(disk);
    if (diskinfo == nil) {
      CFRelease(disk);
      continue;
    }

    NSString *volumeName;
    [path getResourceValue:&volumeName forKey:NSURLVolumeLocalizedNameKey error:nil];

    list.disks = (Disk*)realloc(list.disks, (list.n + 1) * sizeof(Disk));
    list.n++;
    Disk* d = &list.disks[list.n - 1];

    d->bsdname = MakeOwnedCStr(bsdnameChar);

    // For keys, see: https://developer.apple.com/documentation/diskarbitration/diskarbitration_constants

    NSString *description = (NSString*)CFDictionaryGetValue(diskinfo, kDADiskDescriptionMediaNameKey);
    if (description != nil) {
      const char* description_bytes = [description UTF8String];
      d->model = MakeOwnedCStr(description_bytes);
    } else {
      d->model = 0;
    }

    bool removable = [DictionaryGetNumber(diskinfo, kDADiskDescriptionMediaRemovableKey) boolValue];
    bool ejectable = [DictionaryGetNumber(diskinfo, kDADiskDescriptionMediaEjectableKey) boolValue];
    d->is_removable = removable || ejectable;

    d->size_is_known = 1;
    d->size = [DictionaryGetNumber(diskinfo, kDADiskDescriptionMediaSizeKey) unsignedLongValue];

    NSNumber *blockSize = DictionaryGetNumber(diskinfo, kDADiskDescriptionMediaBlockSizeKey);
    d->block_size_is_known = 1;
    d->block_size = [blockSize unsignedLongValue];

    CFRelease(disk);
  }
  CFRelease(session);

  return list;
}
