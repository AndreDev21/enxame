// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint, type=warning, deprecated_member_use, deprecated_member_use_from_same_package
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'app.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$AppEvent {





@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is AppEvent);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'AppEvent()';
}


}

/// @nodoc
class $AppEventCopyWith<$Res>  {
$AppEventCopyWith(AppEvent _, $Res Function(AppEvent) __);
}


/// Adds pattern-matching-related methods to [AppEvent].
extension AppEventPatterns on AppEvent {
/// A variant of `map` that fallback to returning `orElse`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( AppEvent_MessageReceived value)?  messageReceived,TResult Function( AppEvent_ContactRequested value)?  contactRequested,TResult Function( AppEvent_ContactAdded value)?  contactAdded,TResult Function( AppEvent_RoleChanged value)?  roleChanged,TResult Function( AppEvent_Transfer value)?  transfer,TResult Function( AppEvent_GroupJoined value)?  groupJoined,TResult Function( AppEvent_GroupUpdated value)?  groupUpdated,TResult Function( AppEvent_GroupRemoved value)?  groupRemoved,TResult Function( AppEvent_CallSignal value)?  callSignal,TResult Function( AppEvent_Lagged value)?  lagged,required TResult orElse(),}){
final _that = this;
switch (_that) {
case AppEvent_MessageReceived() when messageReceived != null:
return messageReceived(_that);case AppEvent_ContactRequested() when contactRequested != null:
return contactRequested(_that);case AppEvent_ContactAdded() when contactAdded != null:
return contactAdded(_that);case AppEvent_RoleChanged() when roleChanged != null:
return roleChanged(_that);case AppEvent_Transfer() when transfer != null:
return transfer(_that);case AppEvent_GroupJoined() when groupJoined != null:
return groupJoined(_that);case AppEvent_GroupUpdated() when groupUpdated != null:
return groupUpdated(_that);case AppEvent_GroupRemoved() when groupRemoved != null:
return groupRemoved(_that);case AppEvent_CallSignal() when callSignal != null:
return callSignal(_that);case AppEvent_Lagged() when lagged != null:
return lagged(_that);case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// Callbacks receives the raw object, upcasted.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case final Subclass2 value:
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( AppEvent_MessageReceived value)  messageReceived,required TResult Function( AppEvent_ContactRequested value)  contactRequested,required TResult Function( AppEvent_ContactAdded value)  contactAdded,required TResult Function( AppEvent_RoleChanged value)  roleChanged,required TResult Function( AppEvent_Transfer value)  transfer,required TResult Function( AppEvent_GroupJoined value)  groupJoined,required TResult Function( AppEvent_GroupUpdated value)  groupUpdated,required TResult Function( AppEvent_GroupRemoved value)  groupRemoved,required TResult Function( AppEvent_CallSignal value)  callSignal,required TResult Function( AppEvent_Lagged value)  lagged,}){
final _that = this;
switch (_that) {
case AppEvent_MessageReceived():
return messageReceived(_that);case AppEvent_ContactRequested():
return contactRequested(_that);case AppEvent_ContactAdded():
return contactAdded(_that);case AppEvent_RoleChanged():
return roleChanged(_that);case AppEvent_Transfer():
return transfer(_that);case AppEvent_GroupJoined():
return groupJoined(_that);case AppEvent_GroupUpdated():
return groupUpdated(_that);case AppEvent_GroupRemoved():
return groupRemoved(_that);case AppEvent_CallSignal():
return callSignal(_that);case AppEvent_Lagged():
return lagged(_that);}
}
/// A variant of `map` that fallback to returning `null`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( AppEvent_MessageReceived value)?  messageReceived,TResult? Function( AppEvent_ContactRequested value)?  contactRequested,TResult? Function( AppEvent_ContactAdded value)?  contactAdded,TResult? Function( AppEvent_RoleChanged value)?  roleChanged,TResult? Function( AppEvent_Transfer value)?  transfer,TResult? Function( AppEvent_GroupJoined value)?  groupJoined,TResult? Function( AppEvent_GroupUpdated value)?  groupUpdated,TResult? Function( AppEvent_GroupRemoved value)?  groupRemoved,TResult? Function( AppEvent_CallSignal value)?  callSignal,TResult? Function( AppEvent_Lagged value)?  lagged,}){
final _that = this;
switch (_that) {
case AppEvent_MessageReceived() when messageReceived != null:
return messageReceived(_that);case AppEvent_ContactRequested() when contactRequested != null:
return contactRequested(_that);case AppEvent_ContactAdded() when contactAdded != null:
return contactAdded(_that);case AppEvent_RoleChanged() when roleChanged != null:
return roleChanged(_that);case AppEvent_Transfer() when transfer != null:
return transfer(_that);case AppEvent_GroupJoined() when groupJoined != null:
return groupJoined(_that);case AppEvent_GroupUpdated() when groupUpdated != null:
return groupUpdated(_that);case AppEvent_GroupRemoved() when groupRemoved != null:
return groupRemoved(_that);case AppEvent_CallSignal() when callSignal != null:
return callSignal(_that);case AppEvent_Lagged() when lagged != null:
return lagged(_that);case _:
  return null;

}
}
/// A variant of `when` that fallback to an `orElse` callback.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String conversationId,  String fromName,  String fromFingerprint)?  messageReceived,TResult Function( BigInt requestId,  String name,  String fingerprint)?  contactRequested,TResult Function( String name,  String fingerprint)?  contactAdded,TResult Function( String from,  String to)?  roleChanged,TResult Function( String name,  BigInt done,  BigInt total,  bool upload)?  transfer,TResult Function( String groupId,  String name,  String creator,  List<String> exposed)?  groupJoined,TResult Function( String groupId,  String name)?  groupUpdated,TResult Function( String groupId,  String name)?  groupRemoved,TResult Function( String fromFingerprint,  String fromName,  String payload)?  callSignal,TResult Function()?  lagged,required TResult orElse(),}) {final _that = this;
switch (_that) {
case AppEvent_MessageReceived() when messageReceived != null:
return messageReceived(_that.conversationId,_that.fromName,_that.fromFingerprint);case AppEvent_ContactRequested() when contactRequested != null:
return contactRequested(_that.requestId,_that.name,_that.fingerprint);case AppEvent_ContactAdded() when contactAdded != null:
return contactAdded(_that.name,_that.fingerprint);case AppEvent_RoleChanged() when roleChanged != null:
return roleChanged(_that.from,_that.to);case AppEvent_Transfer() when transfer != null:
return transfer(_that.name,_that.done,_that.total,_that.upload);case AppEvent_GroupJoined() when groupJoined != null:
return groupJoined(_that.groupId,_that.name,_that.creator,_that.exposed);case AppEvent_GroupUpdated() when groupUpdated != null:
return groupUpdated(_that.groupId,_that.name);case AppEvent_GroupRemoved() when groupRemoved != null:
return groupRemoved(_that.groupId,_that.name);case AppEvent_CallSignal() when callSignal != null:
return callSignal(_that.fromFingerprint,_that.fromName,_that.payload);case AppEvent_Lagged() when lagged != null:
return lagged();case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// As opposed to `map`, this offers destructuring.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case Subclass2(:final field2):
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String conversationId,  String fromName,  String fromFingerprint)  messageReceived,required TResult Function( BigInt requestId,  String name,  String fingerprint)  contactRequested,required TResult Function( String name,  String fingerprint)  contactAdded,required TResult Function( String from,  String to)  roleChanged,required TResult Function( String name,  BigInt done,  BigInt total,  bool upload)  transfer,required TResult Function( String groupId,  String name,  String creator,  List<String> exposed)  groupJoined,required TResult Function( String groupId,  String name)  groupUpdated,required TResult Function( String groupId,  String name)  groupRemoved,required TResult Function( String fromFingerprint,  String fromName,  String payload)  callSignal,required TResult Function()  lagged,}) {final _that = this;
switch (_that) {
case AppEvent_MessageReceived():
return messageReceived(_that.conversationId,_that.fromName,_that.fromFingerprint);case AppEvent_ContactRequested():
return contactRequested(_that.requestId,_that.name,_that.fingerprint);case AppEvent_ContactAdded():
return contactAdded(_that.name,_that.fingerprint);case AppEvent_RoleChanged():
return roleChanged(_that.from,_that.to);case AppEvent_Transfer():
return transfer(_that.name,_that.done,_that.total,_that.upload);case AppEvent_GroupJoined():
return groupJoined(_that.groupId,_that.name,_that.creator,_that.exposed);case AppEvent_GroupUpdated():
return groupUpdated(_that.groupId,_that.name);case AppEvent_GroupRemoved():
return groupRemoved(_that.groupId,_that.name);case AppEvent_CallSignal():
return callSignal(_that.fromFingerprint,_that.fromName,_that.payload);case AppEvent_Lagged():
return lagged();}
}
/// A variant of `when` that fallback to returning `null`
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String conversationId,  String fromName,  String fromFingerprint)?  messageReceived,TResult? Function( BigInt requestId,  String name,  String fingerprint)?  contactRequested,TResult? Function( String name,  String fingerprint)?  contactAdded,TResult? Function( String from,  String to)?  roleChanged,TResult? Function( String name,  BigInt done,  BigInt total,  bool upload)?  transfer,TResult? Function( String groupId,  String name,  String creator,  List<String> exposed)?  groupJoined,TResult? Function( String groupId,  String name)?  groupUpdated,TResult? Function( String groupId,  String name)?  groupRemoved,TResult? Function( String fromFingerprint,  String fromName,  String payload)?  callSignal,TResult? Function()?  lagged,}) {final _that = this;
switch (_that) {
case AppEvent_MessageReceived() when messageReceived != null:
return messageReceived(_that.conversationId,_that.fromName,_that.fromFingerprint);case AppEvent_ContactRequested() when contactRequested != null:
return contactRequested(_that.requestId,_that.name,_that.fingerprint);case AppEvent_ContactAdded() when contactAdded != null:
return contactAdded(_that.name,_that.fingerprint);case AppEvent_RoleChanged() when roleChanged != null:
return roleChanged(_that.from,_that.to);case AppEvent_Transfer() when transfer != null:
return transfer(_that.name,_that.done,_that.total,_that.upload);case AppEvent_GroupJoined() when groupJoined != null:
return groupJoined(_that.groupId,_that.name,_that.creator,_that.exposed);case AppEvent_GroupUpdated() when groupUpdated != null:
return groupUpdated(_that.groupId,_that.name);case AppEvent_GroupRemoved() when groupRemoved != null:
return groupRemoved(_that.groupId,_that.name);case AppEvent_CallSignal() when callSignal != null:
return callSignal(_that.fromFingerprint,_that.fromName,_that.payload);case AppEvent_Lagged() when lagged != null:
return lagged();case _:
  return null;

}
}

}

/// @nodoc


class AppEvent_MessageReceived extends AppEvent {
  const AppEvent_MessageReceived({required this.conversationId, required this.fromName, required this.fromFingerprint}): super._();
  

 final  String conversationId;
 final  String fromName;
 final  String fromFingerprint;

/// Create a copy of AppEvent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$AppEvent_MessageReceivedCopyWith<AppEvent_MessageReceived> get copyWith => _$AppEvent_MessageReceivedCopyWithImpl<AppEvent_MessageReceived>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is AppEvent_MessageReceived&&(identical(other.conversationId, conversationId) || other.conversationId == conversationId)&&(identical(other.fromName, fromName) || other.fromName == fromName)&&(identical(other.fromFingerprint, fromFingerprint) || other.fromFingerprint == fromFingerprint));
}


@override
int get hashCode {
    return Object.hash(runtimeType,conversationId,fromName,fromFingerprint);
}

@override
String toString() {
    return 'AppEvent.messageReceived(conversationId: $conversationId, fromName: $fromName, fromFingerprint: $fromFingerprint)';
}


}

/// @nodoc
abstract mixin class $AppEvent_MessageReceivedCopyWith<$Res> implements $AppEventCopyWith<$Res> {
  factory $AppEvent_MessageReceivedCopyWith(AppEvent_MessageReceived value, $Res Function(AppEvent_MessageReceived) _then) = _$AppEvent_MessageReceivedCopyWithImpl;
@useResult
$Res call({
 String conversationId, String fromName, String fromFingerprint
});




}
/// @nodoc
class _$AppEvent_MessageReceivedCopyWithImpl<$Res>
    implements $AppEvent_MessageReceivedCopyWith<$Res> {
  _$AppEvent_MessageReceivedCopyWithImpl(this._self, this._then);

  final AppEvent_MessageReceived _self;
  final $Res Function(AppEvent_MessageReceived) _then;

/// Create a copy of AppEvent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? conversationId = null,Object? fromName = null,Object? fromFingerprint = null,}) {
  return _then(AppEvent_MessageReceived(
conversationId: null == conversationId ? _self.conversationId : conversationId // ignore: cast_nullable_to_non_nullable
as String,fromName: null == fromName ? _self.fromName : fromName // ignore: cast_nullable_to_non_nullable
as String,fromFingerprint: null == fromFingerprint ? _self.fromFingerprint : fromFingerprint // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class AppEvent_ContactRequested extends AppEvent {
  const AppEvent_ContactRequested({required this.requestId, required this.name, required this.fingerprint}): super._();
  

 final  BigInt requestId;
 final  String name;
 final  String fingerprint;

/// Create a copy of AppEvent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$AppEvent_ContactRequestedCopyWith<AppEvent_ContactRequested> get copyWith => _$AppEvent_ContactRequestedCopyWithImpl<AppEvent_ContactRequested>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is AppEvent_ContactRequested&&(identical(other.requestId, requestId) || other.requestId == requestId)&&(identical(other.name, name) || other.name == name)&&(identical(other.fingerprint, fingerprint) || other.fingerprint == fingerprint));
}


@override
int get hashCode {
    return Object.hash(runtimeType,requestId,name,fingerprint);
}

@override
String toString() {
    return 'AppEvent.contactRequested(requestId: $requestId, name: $name, fingerprint: $fingerprint)';
}


}

/// @nodoc
abstract mixin class $AppEvent_ContactRequestedCopyWith<$Res> implements $AppEventCopyWith<$Res> {
  factory $AppEvent_ContactRequestedCopyWith(AppEvent_ContactRequested value, $Res Function(AppEvent_ContactRequested) _then) = _$AppEvent_ContactRequestedCopyWithImpl;
@useResult
$Res call({
 BigInt requestId, String name, String fingerprint
});




}
/// @nodoc
class _$AppEvent_ContactRequestedCopyWithImpl<$Res>
    implements $AppEvent_ContactRequestedCopyWith<$Res> {
  _$AppEvent_ContactRequestedCopyWithImpl(this._self, this._then);

  final AppEvent_ContactRequested _self;
  final $Res Function(AppEvent_ContactRequested) _then;

/// Create a copy of AppEvent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? requestId = null,Object? name = null,Object? fingerprint = null,}) {
  return _then(AppEvent_ContactRequested(
requestId: null == requestId ? _self.requestId : requestId // ignore: cast_nullable_to_non_nullable
as BigInt,name: null == name ? _self.name : name // ignore: cast_nullable_to_non_nullable
as String,fingerprint: null == fingerprint ? _self.fingerprint : fingerprint // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class AppEvent_ContactAdded extends AppEvent {
  const AppEvent_ContactAdded({required this.name, required this.fingerprint}): super._();
  

 final  String name;
 final  String fingerprint;

/// Create a copy of AppEvent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$AppEvent_ContactAddedCopyWith<AppEvent_ContactAdded> get copyWith => _$AppEvent_ContactAddedCopyWithImpl<AppEvent_ContactAdded>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is AppEvent_ContactAdded&&(identical(other.name, name) || other.name == name)&&(identical(other.fingerprint, fingerprint) || other.fingerprint == fingerprint));
}


@override
int get hashCode {
    return Object.hash(runtimeType,name,fingerprint);
}

@override
String toString() {
    return 'AppEvent.contactAdded(name: $name, fingerprint: $fingerprint)';
}


}

/// @nodoc
abstract mixin class $AppEvent_ContactAddedCopyWith<$Res> implements $AppEventCopyWith<$Res> {
  factory $AppEvent_ContactAddedCopyWith(AppEvent_ContactAdded value, $Res Function(AppEvent_ContactAdded) _then) = _$AppEvent_ContactAddedCopyWithImpl;
@useResult
$Res call({
 String name, String fingerprint
});




}
/// @nodoc
class _$AppEvent_ContactAddedCopyWithImpl<$Res>
    implements $AppEvent_ContactAddedCopyWith<$Res> {
  _$AppEvent_ContactAddedCopyWithImpl(this._self, this._then);

  final AppEvent_ContactAdded _self;
  final $Res Function(AppEvent_ContactAdded) _then;

/// Create a copy of AppEvent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? name = null,Object? fingerprint = null,}) {
  return _then(AppEvent_ContactAdded(
name: null == name ? _self.name : name // ignore: cast_nullable_to_non_nullable
as String,fingerprint: null == fingerprint ? _self.fingerprint : fingerprint // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class AppEvent_RoleChanged extends AppEvent {
  const AppEvent_RoleChanged({required this.from, required this.to}): super._();
  

 final  String from;
 final  String to;

/// Create a copy of AppEvent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$AppEvent_RoleChangedCopyWith<AppEvent_RoleChanged> get copyWith => _$AppEvent_RoleChangedCopyWithImpl<AppEvent_RoleChanged>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is AppEvent_RoleChanged&&(identical(other.from, from) || other.from == from)&&(identical(other.to, to) || other.to == to));
}


@override
int get hashCode {
    return Object.hash(runtimeType,from,to);
}

@override
String toString() {
    return 'AppEvent.roleChanged(from: $from, to: $to)';
}


}

/// @nodoc
abstract mixin class $AppEvent_RoleChangedCopyWith<$Res> implements $AppEventCopyWith<$Res> {
  factory $AppEvent_RoleChangedCopyWith(AppEvent_RoleChanged value, $Res Function(AppEvent_RoleChanged) _then) = _$AppEvent_RoleChangedCopyWithImpl;
@useResult
$Res call({
 String from, String to
});




}
/// @nodoc
class _$AppEvent_RoleChangedCopyWithImpl<$Res>
    implements $AppEvent_RoleChangedCopyWith<$Res> {
  _$AppEvent_RoleChangedCopyWithImpl(this._self, this._then);

  final AppEvent_RoleChanged _self;
  final $Res Function(AppEvent_RoleChanged) _then;

/// Create a copy of AppEvent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? from = null,Object? to = null,}) {
  return _then(AppEvent_RoleChanged(
from: null == from ? _self.from : from // ignore: cast_nullable_to_non_nullable
as String,to: null == to ? _self.to : to // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class AppEvent_Transfer extends AppEvent {
  const AppEvent_Transfer({required this.name, required this.done, required this.total, required this.upload}): super._();
  

 final  String name;
 final  BigInt done;
 final  BigInt total;
 final  bool upload;

/// Create a copy of AppEvent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$AppEvent_TransferCopyWith<AppEvent_Transfer> get copyWith => _$AppEvent_TransferCopyWithImpl<AppEvent_Transfer>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is AppEvent_Transfer&&(identical(other.name, name) || other.name == name)&&(identical(other.done, done) || other.done == done)&&(identical(other.total, total) || other.total == total)&&(identical(other.upload, upload) || other.upload == upload));
}


@override
int get hashCode {
    return Object.hash(runtimeType,name,done,total,upload);
}

@override
String toString() {
    return 'AppEvent.transfer(name: $name, done: $done, total: $total, upload: $upload)';
}


}

/// @nodoc
abstract mixin class $AppEvent_TransferCopyWith<$Res> implements $AppEventCopyWith<$Res> {
  factory $AppEvent_TransferCopyWith(AppEvent_Transfer value, $Res Function(AppEvent_Transfer) _then) = _$AppEvent_TransferCopyWithImpl;
@useResult
$Res call({
 String name, BigInt done, BigInt total, bool upload
});




}
/// @nodoc
class _$AppEvent_TransferCopyWithImpl<$Res>
    implements $AppEvent_TransferCopyWith<$Res> {
  _$AppEvent_TransferCopyWithImpl(this._self, this._then);

  final AppEvent_Transfer _self;
  final $Res Function(AppEvent_Transfer) _then;

/// Create a copy of AppEvent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? name = null,Object? done = null,Object? total = null,Object? upload = null,}) {
  return _then(AppEvent_Transfer(
name: null == name ? _self.name : name // ignore: cast_nullable_to_non_nullable
as String,done: null == done ? _self.done : done // ignore: cast_nullable_to_non_nullable
as BigInt,total: null == total ? _self.total : total // ignore: cast_nullable_to_non_nullable
as BigInt,upload: null == upload ? _self.upload : upload // ignore: cast_nullable_to_non_nullable
as bool,
  ));
}


}

/// @nodoc


class AppEvent_GroupJoined extends AppEvent {
  const AppEvent_GroupJoined({required this.groupId, required this.name, required this.creator, required  List<String> exposed}): _exposed = exposed,super._();
  

 final  String groupId;
 final  String name;
 final  String creator;
 final  List<String> _exposed;
 List<String> get exposed {
  if (_exposed is EqualUnmodifiableListView) return _exposed;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_exposed);
}


/// Create a copy of AppEvent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$AppEvent_GroupJoinedCopyWith<AppEvent_GroupJoined> get copyWith => _$AppEvent_GroupJoinedCopyWithImpl<AppEvent_GroupJoined>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is AppEvent_GroupJoined&&(identical(other.groupId, groupId) || other.groupId == groupId)&&(identical(other.name, name) || other.name == name)&&(identical(other.creator, creator) || other.creator == creator)&&const DeepCollectionEquality().equals(other.exposed, _exposed));
}


@override
int get hashCode {
    return Object.hash(runtimeType,groupId,name,creator,const DeepCollectionEquality().hash(_exposed));
}

@override
String toString() {
    return 'AppEvent.groupJoined(groupId: $groupId, name: $name, creator: $creator, exposed: $exposed)';
}


}

/// @nodoc
abstract mixin class $AppEvent_GroupJoinedCopyWith<$Res> implements $AppEventCopyWith<$Res> {
  factory $AppEvent_GroupJoinedCopyWith(AppEvent_GroupJoined value, $Res Function(AppEvent_GroupJoined) _then) = _$AppEvent_GroupJoinedCopyWithImpl;
@useResult
$Res call({
 String groupId, String name, String creator, List<String> exposed
});




}
/// @nodoc
class _$AppEvent_GroupJoinedCopyWithImpl<$Res>
    implements $AppEvent_GroupJoinedCopyWith<$Res> {
  _$AppEvent_GroupJoinedCopyWithImpl(this._self, this._then);

  final AppEvent_GroupJoined _self;
  final $Res Function(AppEvent_GroupJoined) _then;

/// Create a copy of AppEvent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? groupId = null,Object? name = null,Object? creator = null,Object? exposed = null,}) {
  return _then(AppEvent_GroupJoined(
groupId: null == groupId ? _self.groupId : groupId // ignore: cast_nullable_to_non_nullable
as String,name: null == name ? _self.name : name // ignore: cast_nullable_to_non_nullable
as String,creator: null == creator ? _self.creator : creator // ignore: cast_nullable_to_non_nullable
as String,exposed: null == exposed ? _self._exposed : exposed // ignore: cast_nullable_to_non_nullable
as List<String>,
  ));
}


}

/// @nodoc


class AppEvent_GroupUpdated extends AppEvent {
  const AppEvent_GroupUpdated({required this.groupId, required this.name}): super._();
  

 final  String groupId;
 final  String name;

/// Create a copy of AppEvent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$AppEvent_GroupUpdatedCopyWith<AppEvent_GroupUpdated> get copyWith => _$AppEvent_GroupUpdatedCopyWithImpl<AppEvent_GroupUpdated>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is AppEvent_GroupUpdated&&(identical(other.groupId, groupId) || other.groupId == groupId)&&(identical(other.name, name) || other.name == name));
}


@override
int get hashCode {
    return Object.hash(runtimeType,groupId,name);
}

@override
String toString() {
    return 'AppEvent.groupUpdated(groupId: $groupId, name: $name)';
}


}

/// @nodoc
abstract mixin class $AppEvent_GroupUpdatedCopyWith<$Res> implements $AppEventCopyWith<$Res> {
  factory $AppEvent_GroupUpdatedCopyWith(AppEvent_GroupUpdated value, $Res Function(AppEvent_GroupUpdated) _then) = _$AppEvent_GroupUpdatedCopyWithImpl;
@useResult
$Res call({
 String groupId, String name
});




}
/// @nodoc
class _$AppEvent_GroupUpdatedCopyWithImpl<$Res>
    implements $AppEvent_GroupUpdatedCopyWith<$Res> {
  _$AppEvent_GroupUpdatedCopyWithImpl(this._self, this._then);

  final AppEvent_GroupUpdated _self;
  final $Res Function(AppEvent_GroupUpdated) _then;

/// Create a copy of AppEvent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? groupId = null,Object? name = null,}) {
  return _then(AppEvent_GroupUpdated(
groupId: null == groupId ? _self.groupId : groupId // ignore: cast_nullable_to_non_nullable
as String,name: null == name ? _self.name : name // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class AppEvent_GroupRemoved extends AppEvent {
  const AppEvent_GroupRemoved({required this.groupId, required this.name}): super._();
  

 final  String groupId;
 final  String name;

/// Create a copy of AppEvent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$AppEvent_GroupRemovedCopyWith<AppEvent_GroupRemoved> get copyWith => _$AppEvent_GroupRemovedCopyWithImpl<AppEvent_GroupRemoved>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is AppEvent_GroupRemoved&&(identical(other.groupId, groupId) || other.groupId == groupId)&&(identical(other.name, name) || other.name == name));
}


@override
int get hashCode {
    return Object.hash(runtimeType,groupId,name);
}

@override
String toString() {
    return 'AppEvent.groupRemoved(groupId: $groupId, name: $name)';
}


}

/// @nodoc
abstract mixin class $AppEvent_GroupRemovedCopyWith<$Res> implements $AppEventCopyWith<$Res> {
  factory $AppEvent_GroupRemovedCopyWith(AppEvent_GroupRemoved value, $Res Function(AppEvent_GroupRemoved) _then) = _$AppEvent_GroupRemovedCopyWithImpl;
@useResult
$Res call({
 String groupId, String name
});




}
/// @nodoc
class _$AppEvent_GroupRemovedCopyWithImpl<$Res>
    implements $AppEvent_GroupRemovedCopyWith<$Res> {
  _$AppEvent_GroupRemovedCopyWithImpl(this._self, this._then);

  final AppEvent_GroupRemoved _self;
  final $Res Function(AppEvent_GroupRemoved) _then;

/// Create a copy of AppEvent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? groupId = null,Object? name = null,}) {
  return _then(AppEvent_GroupRemoved(
groupId: null == groupId ? _self.groupId : groupId // ignore: cast_nullable_to_non_nullable
as String,name: null == name ? _self.name : name // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class AppEvent_CallSignal extends AppEvent {
  const AppEvent_CallSignal({required this.fromFingerprint, required this.fromName, required this.payload}): super._();
  

 final  String fromFingerprint;
 final  String fromName;
 final  String payload;

/// Create a copy of AppEvent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$AppEvent_CallSignalCopyWith<AppEvent_CallSignal> get copyWith => _$AppEvent_CallSignalCopyWithImpl<AppEvent_CallSignal>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is AppEvent_CallSignal&&(identical(other.fromFingerprint, fromFingerprint) || other.fromFingerprint == fromFingerprint)&&(identical(other.fromName, fromName) || other.fromName == fromName)&&(identical(other.payload, payload) || other.payload == payload));
}


@override
int get hashCode {
    return Object.hash(runtimeType,fromFingerprint,fromName,payload);
}

@override
String toString() {
    return 'AppEvent.callSignal(fromFingerprint: $fromFingerprint, fromName: $fromName, payload: $payload)';
}


}

/// @nodoc
abstract mixin class $AppEvent_CallSignalCopyWith<$Res> implements $AppEventCopyWith<$Res> {
  factory $AppEvent_CallSignalCopyWith(AppEvent_CallSignal value, $Res Function(AppEvent_CallSignal) _then) = _$AppEvent_CallSignalCopyWithImpl;
@useResult
$Res call({
 String fromFingerprint, String fromName, String payload
});




}
/// @nodoc
class _$AppEvent_CallSignalCopyWithImpl<$Res>
    implements $AppEvent_CallSignalCopyWith<$Res> {
  _$AppEvent_CallSignalCopyWithImpl(this._self, this._then);

  final AppEvent_CallSignal _self;
  final $Res Function(AppEvent_CallSignal) _then;

/// Create a copy of AppEvent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? fromFingerprint = null,Object? fromName = null,Object? payload = null,}) {
  return _then(AppEvent_CallSignal(
fromFingerprint: null == fromFingerprint ? _self.fromFingerprint : fromFingerprint // ignore: cast_nullable_to_non_nullable
as String,fromName: null == fromName ? _self.fromName : fromName // ignore: cast_nullable_to_non_nullable
as String,payload: null == payload ? _self.payload : payload // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class AppEvent_Lagged extends AppEvent {
  const AppEvent_Lagged(): super._();
  






@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is AppEvent_Lagged);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'AppEvent.lagged()';
}


}




// dart format on
